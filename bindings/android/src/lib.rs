use jni::JNIEnv;
use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jstring, jbyteArray};
use std::path::PathBuf;
use writer_core::facade::WriterCore;
use serde::Serialize;
use serde_json::json;

// Helper to convert JString to Rust String
fn jstring_to_string(env: &mut JNIEnv, jstr: &JString) -> Result<String, String> {
    if jstr.is_null() {
        return Ok(String::new());
    }
    env.get_string(jstr)
        .map(|s| s.into())
        .map_err(|e| format!("Failed to get string: {}", e))
}

// Helper to return JSON or error JSON
fn result_to_jstring<T: Serialize>(env: &mut JNIEnv, result: Result<T, writer_core::Error>) -> jstring {
    let json_str = match result {
        Ok(data) => {
            json!({
                "success": true,
                "data": data
            }).to_string()
        },
        Err(e) => {
            json!({
                "success": false,
                "error": e.to_string()
            }).to_string()
        }
    };

    match env.new_string(json_str) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut()
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
        Ok(valid) => if valid { 1 } else { 0 },
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
    result_to_jstring(&mut env, core.create_chapter(&project_id, &volume_id, &title))
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
    result_to_jstring(&mut env, core.read_chapter(&project_id, &volume_id, &chapter_id))
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
) -> jboolean {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
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
    let content = match jstring_to_string(&mut env, &content_j) {
        Ok(s) => s,
        Err(_) => return 0,
    };

    let core = WriterCore::new(&workspace_path);
    match core.write_chapter(&project_id, &volume_id, &chapter_id, &content) {
        Ok(_) => 1,
        Err(_) => 0,
    }
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
) -> jboolean {
    let workspace_path = match jstring_to_string(&mut env, &workspace_path_j) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let settings_json = match jstring_to_string(&mut env, &settings_json_j) {
        Ok(s) => s,
        Err(_) => return 0,
    };

    let settings = match serde_json::from_str(&settings_json) {
        Ok(s) => s,
        Err(_) => return 0,
    };

    let core = WriterCore::new(&workspace_path);
    match core.save_local_settings(&settings) {
        Ok(_) => 1,
        Err(_) => 0,
    }
}
