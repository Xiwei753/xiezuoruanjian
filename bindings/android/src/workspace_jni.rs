use super::*;

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

    let api = api_from_workspace(&workspace_path);
    match api.create_workspace_if_needed() {
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

    let api = api_from_workspace(&workspace_path);
    match api.validate_workspace() {
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
