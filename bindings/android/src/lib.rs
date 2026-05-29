//! # Android JNI 桥接层（Binding 层）
//!
//! 本 crate 是 Android 客户端与 Rust Core 之间的桥梁。
//!
//! ## 架构定位
//!
//! ```text
//! Android Kotlin UI → 领域 Bridge → NativeCoreBridge.kt → JNI → WriterCore (Core 层)
//! ```
//!
//! ## 职责边界
//!
//! - **做**：JString ↔ String 转换、Result → 兼容 DTO 序列化、JNI 函数注册
//! - **不做**：业务逻辑（全部委托给 `WriterCore`）
//! - **不做**：错误处理（错误通过稳定 code/message 返回给 Kotlin）
//!
//! ## 兼容协议
//!
//! 旧 JNI 函数仍返回 JSON 字符串，但错误必须包含稳定 code：
//! - 成功：`{ "success": true, "data": <实际数据> }`
//! - 失败：`{ "success": false, "code": "...", "error": "<错误信息>" }`
//!
//! ## 注意事项
//!
//! - 本 crate 不允许添加业务逻辑，只做类型转换
//! - 所有 `WriterCore` 方法调用都在这里，便于追踪调用链

use jni::objects::JObject;
use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jstring};
use jni::JNIEnv;
use serde::Serialize;
use writer_core::facade::WriterCore;

/// 将 JString 转换为 Rust String。
///
/// 如果 JString 为 null，返回空字符串。
fn jstring_to_string(env: &mut JNIEnv, jstr: &JString) -> Result<String, String> {
    if jstr.is_null() {
        return Ok(String::new());
    }
    env.get_string(jstr)
        .map(|s| s.into())
        .map_err(|e| format!("Failed to get string: {}", e))
}

/// 将 Core 层 Result 转换为 JSON 字符串返回给 Kotlin。
///
/// 成功时返回 `{ "success": true, "data": ... }`
/// 失败时返回 `{ "success": false, "code": "...", "error": "..." }`
fn result_to_jstring<T: Serialize>(
    env: &mut JNIEnv,
    result: Result<T, writer_core::Error>,
) -> jstring {
    let json_str = serde_json::to_string(&writer_core::error::BridgeResult::from_result(result))
        .unwrap_or_else(|e| {
            format!(
                r#"{{"success":false,"code":"JSON_ERROR","error":"Failed to serialize bridge result: {}"}}"#,
                e
            )
        });

    match env.new_string(json_str) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

// Create Workspace
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_createWorkspace(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
) -> jboolean {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return 0,
    };

    let core = WriterCore::new(&workspace_path);
    match core.create_workspace() {
        Ok(_) => 1,
        Err(_) => 0,
    }
}

// Validate Workspace
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_validateWorkspace(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
) -> jboolean {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return 0,
    };

    let core = WriterCore::new(&workspace_path);
    match core.validate_workspace() {
        Ok(valid) => {
            if valid {
                1
            } else {
                0
            }
        }
        Err(_) => 0,
    }
}

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

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.list_projects())
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

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.get_recent_edits())
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

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(
        &mut env,
        core.record_recent_edit(&project_id, &volume_id, &chapter_id),
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

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.get_project_stats(&project_id))
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

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.create_project(&title))
}

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

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.list_volumes(&project_id))
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

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.create_volume(&project_id, &title))
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

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.list_chapters(&project_id, &volume_id))
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

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(
        &mut env,
        core.create_chapter(&project_id, &volume_id, &title),
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

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(
        &mut env,
        core.open_chapter(&project_id, &volume_id, &chapter_id),
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

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(
        &mut env,
        core.write_chapter_verified(&project_id, &volume_id, &chapter_id, &content),
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

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(
        &mut env,
        core.clear_chapter_content_verified(&project_id, &volume_id, &chapter_id),
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

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(
        &mut env,
        core.update_chapter_note(&project_id, &volume_id, &chapter_id, &note),
    )
}

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

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.load_local_settings())
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

    let settings = match serde_json::from_str(&settings_json) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::Error::Json(e))),
    };

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.save_local_settings(&settings))
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

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.load_syncable_settings())
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

    let settings = match serde_json::from_str(&settings_json) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::Error::Json(e))),
    };

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.save_syncable_settings(&settings))
}

// --- Sync Service JNI ---

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

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.load_sync_config())
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

    let config = match serde_json::from_str(&config_json) {
        Ok(c) => c,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::Error::Json(e))),
    };

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.save_sync_config(&config))
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

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.load_sync_secrets())
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

    let secrets = match serde_json::from_str(&secrets_json) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::Error::Json(e))),
    };

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.save_sync_secrets(&secrets))
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

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.load_sync_state())
}

// Perform Sync Dry Run
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

    let config = match serde_json::from_str(&config_json) {
        Ok(c) => c,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::Error::Json(e))),
    };

    let core = WriterCore::new(&workspace_path);
    let result = core.perform_sync_diagnostics(&config);
    result_to_jstring(&mut env, result)
}

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

    let config = match serde_json::from_str(&config_json) {
        Ok(c) => c,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::Error::Json(e))),
    };

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.perform_sync_dry_run(&config))
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

    let config = match serde_json::from_str(&config_json) {
        Ok(c) => c,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::Error::Json(e))),
    };

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.perform_sync(&config))
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

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.rename_project(&project_id, &new_title))
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

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.delete_project(&project_id))
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
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::Error::Json(e))),
    };

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.reorder_projects(&ordered_ids))
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

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(
        &mut env,
        core.rename_volume(&project_id, &volume_id, &new_title),
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

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.delete_volume(&project_id, &volume_id))
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
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::Error::Json(e))),
    };

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.reorder_volumes(&project_id, &ordered_ids))
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

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(
        &mut env,
        core.rename_chapter(&project_id, &volume_id, &chapter_id, &new_title),
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

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(
        &mut env,
        core.delete_chapter(&project_id, &volume_id, &chapter_id),
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
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::Error::Json(e))),
    };

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(
        &mut env,
        core.reorder_chapters(&project_id, &volume_id, &ordered_ids),
    )
}

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

    let core = WriterCore::new(&workspace_path);
    let result = core.get_mind_map_snapshot(&project_id);
    result_to_jstring(&mut env, result)
}

fn result_to_jstring_unified<T: Serialize>(
    env: &mut JNIEnv,
    result: Result<T, writer_core::Error>,
) -> jstring {
    let json_str = match result {
        Ok(data) => serde_json::json!({
            "success": true,
            "data": data,
            "error": serde_json::Value::Null
        })
        .to_string(),
        Err(e) => serde_json::json!({
            "success": false,
            "data": serde_json::Value::Null,
            "error": e.to_string()
        })
        .to_string(),
    };

    match env.new_string(json_str) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
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
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let title = match jstring_to_string(&mut env, &title_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };

    let core = WriterCore::new(&workspace_path);
    let result = core.create_mind_map_graph(&project_id, &title);
    result_to_jstring_unified(&mut env, result)
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
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };

    let core = WriterCore::new(&workspace_path);
    let result = core.list_mind_map_graphs(&project_id);
    result_to_jstring_unified(&mut env, result)
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
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let graph_id = match jstring_to_string(&mut env, &graph_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };

    let core = WriterCore::new(&workspace_path);
    let result = core.set_default_mind_map_graph(&project_id, &graph_id);
    result_to_jstring_unified(&mut env, result)
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
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let graph_id = match jstring_to_string(&mut env, &graph_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let node_json = match jstring_to_string(&mut env, &node_json_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };

    let node = match serde_json::from_str(&node_json) {
        Ok(n) => n,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Json(e))),
    };

    let core = WriterCore::new(&workspace_path);
    let result = core.create_mind_map_node(&project_id, &graph_id, node);
    result_to_jstring_unified(&mut env, result)
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
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let graph_id = match jstring_to_string(&mut env, &graph_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let node_id = match jstring_to_string(&mut env, &node_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let patch_json = match jstring_to_string(&mut env, &patch_json_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };

    let patch = match serde_json::from_str(&patch_json) {
        Ok(p) => p,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Json(e))),
    };

    let core = WriterCore::new(&workspace_path);
    let result = core.update_mind_map_node(&project_id, &graph_id, &node_id, patch);
    result_to_jstring_unified(&mut env, result)
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
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let graph_id = match jstring_to_string(&mut env, &graph_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let node_id = match jstring_to_string(&mut env, &node_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };

    let cascade = cascade_j != 0;
    let core = WriterCore::new(&workspace_path);
    let result = core.delete_mind_map_node(&project_id, &graph_id, &node_id, cascade);
    result_to_jstring_unified(&mut env, result)
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
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let graph_id = match jstring_to_string(&mut env, &graph_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let edge_json = match jstring_to_string(&mut env, &edge_json_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };

    let edge = match serde_json::from_str(&edge_json) {
        Ok(ed) => ed,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Json(e))),
    };

    let core = WriterCore::new(&workspace_path);
    let result = core.create_mind_map_edge(&project_id, &graph_id, edge);
    result_to_jstring_unified(&mut env, result)
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
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let graph_id = match jstring_to_string(&mut env, &graph_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let edge_id = match jstring_to_string(&mut env, &edge_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let patch_json = match jstring_to_string(&mut env, &patch_json_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };

    let patch = match serde_json::from_str(&patch_json) {
        Ok(p) => p,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Json(e))),
    };

    let core = WriterCore::new(&workspace_path);
    let result = core.update_mind_map_edge(&project_id, &graph_id, &edge_id, patch);
    result_to_jstring_unified(&mut env, result)
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
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let graph_id = match jstring_to_string(&mut env, &graph_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let edge_id = match jstring_to_string(&mut env, &edge_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };

    let core = WriterCore::new(&workspace_path);
    let result = core.delete_mind_map_edge(&project_id, &graph_id, &edge_id);
    result_to_jstring_unified(&mut env, result)
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
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let graph_id = match jstring_to_string(&mut env, &graph_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let anchor_json = match jstring_to_string(&mut env, &anchor_json_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };

    let anchor = match serde_json::from_str(&anchor_json) {
        Ok(an) => an,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Json(e))),
    };

    let core = WriterCore::new(&workspace_path);
    let result = core.create_mind_map_anchor(&project_id, &graph_id, anchor);
    result_to_jstring_unified(&mut env, result)
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
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let graph_id = match jstring_to_string(&mut env, &graph_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let node_id = match jstring_to_string(&mut env, &node_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let anchor_id = match jstring_to_string(&mut env, &anchor_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let link_kind = match jstring_to_string(&mut env, &link_kind_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };

    let core = WriterCore::new(&workspace_path);
    let result = core.bind_mind_map_node_to_anchor(&project_id, &graph_id, &node_id, &anchor_id, &link_kind);
    result_to_jstring_unified(&mut env, result)
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
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let project_id = match jstring_to_string(&mut env, &project_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let graph_id = match jstring_to_string(&mut env, &graph_id_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let layout_json = match jstring_to_string(&mut env, &layout_json_j) {
        Ok(s) => s,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };

    let layout = match serde_json::from_str(&layout_json) {
        Ok(la) => la,
        Err(e) => return result_to_jstring_unified::<()>(&mut env, Err(writer_core::Error::Json(e))),
    };

    let core = WriterCore::new(&workspace_path);
    let result = core.save_mind_map_layout(&project_id, &graph_id, layout);
    result_to_jstring_unified(&mut env, result)
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_listRegisteredActionsNative(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path: JString,
) -> jstring {
    let ws_path = match jstring_to_string(&mut env, &workspace_path) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };

    let core = WriterCore::new(&ws_path);
    let result = core.list_registered_actions();
    result_to_jstring(&mut env, result)
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_executeActionNative(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path: JString,
    action_id: JString,
    args_json: JString,
    context_json: JString,
) -> jstring {
    let ws_path = match jstring_to_string(&mut env, &workspace_path) {
        Ok(s) => s,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))),
    };
    let act_id = match jstring_to_string(&mut env, &action_id) {
        Ok(s) => s,
        Err(_) => String::new(),
    };
    let args = match jstring_to_string(&mut env, &args_json) {
        Ok(s) => s,
        Err(_) => String::new(),
    };
    let ctx = match jstring_to_string(&mut env, &context_json) {
        Ok(s) => s,
        Err(_) => String::new(),
    };

    let core = WriterCore::new(&ws_path);
    let result = core.execute_action(&act_id, &args, &ctx);

    // We want to return success = true on the RustResponse wrapper if the JNI call itself was successful.
    // ActionResult inside result already contains its own "success" flag which NativeCoreBridge will parse.
    result_to_jstring(&mut env, result)
}

// Check if AI is available (compile-time feature gate)
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_aiAvailableNative(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
) -> jboolean {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return 0,
    };

    let core = WriterCore::new(&workspace_path);
    if core.ai_available() { 1 } else { 0 }
}

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

    let core = WriterCore::new(&workspace_path);
    match core.record_writing_event(
        &device_id,
        "android",
        &project_id,
        &volume_id,
        &chapter_id,
        &source,
        inserted_chars as u32,
        deleted_chars as u32,
        pasted_chars as u32,
        ai_inserted_chars as u32,
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

    let core = WriterCore::new(&workspace_path);
    let _ = core.flush_writing_stats();
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

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(
        &mut env,
        core.get_writing_stats_summary(&start_date, &end_date),
    )
}


// StarMap Endpoints
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_listStarmapsJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.list_starmaps())
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_createStarmapJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    title_j: JString,
    desc_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let title = match jstring_to_string(&mut env, &title_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let desc = match jstring_to_string(&mut env, &desc_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.create_starmap(&title, &desc, None))
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_getStarmapGraphJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    starmap_id_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let starmap_id = match jstring_to_string(&mut env, &starmap_id_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let core = WriterCore::new(&workspace_path);
    let graph_res = core.get_starmap_graph(&starmap_id);
    let layout_res = core.get_starmap_layout(&starmap_id);
    match (graph_res, layout_res) {
        (Ok(graph), Ok(layout)) => {
            let data = serde_json::json!({ "graph": graph, "layout": layout });
            result_to_jstring(&mut env, Ok(data))
        },
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
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let starmap_id = match jstring_to_string(&mut env, &starmap_id_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let node_json = match jstring_to_string(&mut env, &node_json_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let core = WriterCore::new(&workspace_path);
    // Currently Android Canvas drops node anywhere, default it to 0,0 since canvas isn't tracking click coordinates here yet.
    // The core will default it and save to the layout.
    match serde_json::from_str(&node_json) {
        Ok(node) => result_to_jstring(&mut env, core.add_starmap_node(&starmap_id, node, 0.0, 0.0)),
        Err(e) => result_to_jstring::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))))
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
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let starmap_id = match jstring_to_string(&mut env, &starmap_id_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let layout_json = match jstring_to_string(&mut env, &layout_json_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let core = WriterCore::new(&workspace_path);
    match serde_json::from_str(&layout_json) {
        Ok(layout) => result_to_jstring(&mut env, core.save_starmap_layout(&starmap_id, &layout)),
        Err(e) => result_to_jstring::<()>(&mut env, Err(writer_core::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))))
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
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let starmap_id = match jstring_to_string(&mut env, &starmap_id_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let new_title = match jstring_to_string(&mut env, &new_title_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.rename_starmap(&starmap_id, &new_title).map(|_| ()))
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_deleteStarmapNative(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    starmap_id_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let starmap_id = match jstring_to_string(&mut env, &starmap_id_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.delete_starmap(&starmap_id))
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_bindStarmapToProjectJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    starmap_id_j: JString,
    project_id_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let starmap_id = match jstring_to_string(&mut env, &starmap_id_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let project_id = match jstring_to_string(&mut env, &project_id_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.bind_starmap_to_project(&starmap_id, &project_id))
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_unbindStarmapFromProjectJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    starmap_id_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let starmap_id = match jstring_to_string(&mut env, &starmap_id_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.unbind_starmap_from_project(&starmap_id))
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_setMainStarmapForProjectJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    starmap_id_j: JString,
    project_id_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let starmap_id = match jstring_to_string(&mut env, &starmap_id_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let project_id = match jstring_to_string(&mut env, &project_id_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.set_main_starmap_for_project(&starmap_id, &project_id))
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_getMainStarmapForProjectJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    project_id_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let project_id = match jstring_to_string(&mut env, &project_id_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.get_main_starmap_for_project(&project_id))
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
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let parent_id = match jstring_to_string(&mut env, &parent_id_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let title = match jstring_to_string(&mut env, &title_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let desc = match jstring_to_string(&mut env, &desc_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.create_child_starmap_legacy(&parent_id, &title, &desc, None))
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
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let starmap_id = match jstring_to_string(&mut env, &starmap_id_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let node_id = match jstring_to_string(&mut env, &node_id_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let patch_json = match jstring_to_string(&mut env, &patch_json_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };

    let patch: writer_core::starmap::types::StarMapNodePatch = match serde_json::from_str(&patch_json) {
        Ok(p) => p,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::Error::Json(e))),
    };

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.update_starmap_node(&starmap_id, &node_id, patch))
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_deleteStarmapNodeJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    starmap_id_j: JString,
    node_id_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let starmap_id = match jstring_to_string(&mut env, &starmap_id_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let node_id = match jstring_to_string(&mut env, &node_id_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.delete_starmap_node(&starmap_id, &node_id))
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
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let starmap_id = match jstring_to_string(&mut env, &starmap_id_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let edge_json = match jstring_to_string(&mut env, &edge_json_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let edge = match serde_json::from_str(&edge_json) {
        Ok(e) => e,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::Error::Json(e))),
    };
    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.add_starmap_edge(&starmap_id, edge))
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
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let starmap_id = match jstring_to_string(&mut env, &starmap_id_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let edge_id = match jstring_to_string(&mut env, &edge_id_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let patch_json = match jstring_to_string(&mut env, &patch_json_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };

    let patch: writer_core::starmap::types::StarMapEdgePatch = match serde_json::from_str(&patch_json) {
        Ok(p) => p,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::Error::Json(e))),
    };

    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.update_starmap_edge(&starmap_id, &edge_id, patch))
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_deleteStarmapEdgeJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    starmap_id_j: JString,
    edge_id_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let starmap_id = match jstring_to_string(&mut env, &starmap_id_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let edge_id = match jstring_to_string(&mut env, &edge_id_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.delete_starmap_edge(&starmap_id, &edge_id))
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_saveStarmapGraphJson(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    starmap_id_j: JString,
    graph_json_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let starmap_id = match jstring_to_string(&mut env, &starmap_id_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let graph_json = match jstring_to_string(&mut env, &graph_json_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let graph = match serde_json::from_str(&graph_json) {
        Ok(g) => g,
        Err(e) => return result_to_jstring::<()>(&mut env, Err(writer_core::Error::Json(e))),
    };
    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.save_starmap_graph(&starmap_id, &graph))
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
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let start_date = match jstring_to_string(&mut env, &start_date_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let end_date = match jstring_to_string(&mut env, &end_date_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.get_writing_stats_by_project(&start_date, &end_date))
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_getWritingStatsByChapterNative(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    start_date_j: JString,
    end_date_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let start_date = match jstring_to_string(&mut env, &start_date_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let end_date = match jstring_to_string(&mut env, &end_date_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.get_writing_stats_by_chapter(&start_date, &end_date))
}

#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_getWritingStatsByDeviceNative(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path_j: JString,
    start_date_j: JString,
    end_date_j: JString,
) -> jstring {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let start_date = match jstring_to_string(&mut env, &start_date_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let end_date = match jstring_to_string(&mut env, &end_date_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.get_writing_stats_by_device(&start_date, &end_date))
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
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let start_date = match jstring_to_string(&mut env, &start_date_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let end_date = match jstring_to_string(&mut env, &end_date_j) { Ok(s) => s, Err(_) => return std::ptr::null_mut() };
    let core = WriterCore::new(&workspace_path);
    result_to_jstring(&mut env, core.get_writing_speed_curve(&start_date, &end_date, bucket_minutes as u32))
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

    let core = writer_core::facade::WriterCore::new(&workspace_path);
    match core.process_writing_event(
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
