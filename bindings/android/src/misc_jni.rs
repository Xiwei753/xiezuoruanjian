use super::*;

// List Registered Actions
#[no_mangle]
pub extern "system" fn Java_com_xiwei_writerapp_data_NativeCoreBridge_listRegisteredActionsNative(
    mut env: JNIEnv,
    _class: JClass,
    workspace_path: JString,
) -> jstring {
    let ws_path = match jstring_to_string(&mut env, &workspace_path) {
        Ok(s) => s,
        Err(e) => {
            return result_to_jstring::<()>(
                &mut env,
                Err(writer_core::api::error::WriterError::Io(e.to_string())),
            )
        }
    };

    let api = api_from_workspace(&ws_path);
    let result = api.list_registered_actions();
    result_to_jstring(&mut env, result)
}

// Execute Action
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
        Err(e) => {
            return result_to_jstring::<()>(
                &mut env,
                Err(writer_core::api::error::WriterError::Io(e.to_string())),
            )
        }
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

    let api = api_from_workspace(&ws_path);
    let result = api.execute_action_ext(&act_id, &args, &ctx);

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

    let api = api_from_workspace(&workspace_path);
    if api.ai_available() {
        1
    } else {
        0
    }
}
