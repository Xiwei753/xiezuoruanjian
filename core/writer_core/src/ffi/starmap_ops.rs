use std::os::raw::c_char;

use super::{c_str_to_rust, err_json, ok_json, with_app_service};
use crate::api::StarMapMetaDto;

#[no_mangle]
/// # Safety
///
/// This function does not take any pointer arguments, so there are no additional
/// safety requirements beyond those inherent to FFI boundary calls.
pub unsafe extern "C" fn writer_core_list_starmaps() -> *mut c_char {
    match with_app_service(|svc| {
        let starmaps = svc.list_starmaps().map_err(|e| format!("{}", e))?;
        Ok(starmaps)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NOT_FOUND", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// The caller must ensure `project_id` points to a valid, null-terminated C string.
/// Passing a null pointer or an invalid pointer is undefined behavior.
pub unsafe extern "C" fn writer_core_list_starmaps_for_project(
    project_id: *const c_char,
) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid project_id: error {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        let starmaps = svc
            .list_starmaps_for_project(&pid)
            .map_err(|e| format!("{}", e))?;
        let dtos: Vec<StarMapMetaDto> = starmaps.into_iter().map(StarMapMetaDto::from).collect();
        Ok(dtos)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NOT_FOUND", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// The caller must ensure `starmap_id` points to a valid, null-terminated C string.
/// Passing a null pointer or an invalid pointer is undefined behavior.
pub unsafe extern "C" fn writer_core_get_starmap(starmap_id: *const c_char) -> *mut c_char {
    let sid = match c_str_to_rust(starmap_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid starmap_id: error {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        let sm = svc.get_starmap(&sid).map_err(|e| format!("{}", e))?;
        Ok(StarMapMetaDto::from(sm))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NOT_FOUND", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// The caller must ensure `starmap_id` points to a valid, null-terminated C string.
/// Passing a null pointer or an invalid pointer is undefined behavior.
pub unsafe extern "C" fn writer_core_get_starmap_graph(starmap_id: *const c_char) -> *mut c_char {
    let sid = match c_str_to_rust(starmap_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid starmap_id: error {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        let graph = svc.get_starmap_graph(sid).map_err(|e| format!("{}", e))?;
        Ok(graph)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NOT_FOUND", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// The caller must ensure `title` and `description` point to valid, null-terminated C strings.
/// Passing null pointers or invalid pointers is undefined behavior.
pub unsafe extern "C" fn writer_core_create_starmap(
    title: *const c_char,
    description: *const c_char,
) -> *mut c_char {
    let t = match c_str_to_rust(title) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid title: error {}", e)),
    };
    let d = match c_str_to_rust(description) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid description: error {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        let sm = svc.create_starmap(t, d).map_err(|e| format!("{}", e))?;
        Ok(sm)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_ALREADY_EXISTS", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// The caller must ensure `starmap_id` points to a valid, null-terminated C string.
/// Passing a null pointer or an invalid pointer is undefined behavior.
pub unsafe extern "C" fn writer_core_delete_starmap(starmap_id: *const c_char) -> *mut c_char {
    let sid = match c_str_to_rust(starmap_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid starmap_id: error {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        svc.delete_starmap_raw(&sid).map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NOT_FOUND", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// The caller must ensure both `starmap_id` and `new_title` point to valid,
/// null-terminated C strings. Passing null pointers or invalid pointers is undefined behavior.
pub unsafe extern "C" fn writer_core_rename_starmap(
    starmap_id: *const c_char,
    new_title: *const c_char,
) -> *mut c_char {
    let sid = match c_str_to_rust(starmap_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid starmap_id: error {}", e),
            )
        }
    };
    let t = match c_str_to_rust(new_title) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid new_title: error {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        let sm = svc
            .rename_starmap_raw(&sid, &t)
            .map_err(|e| format!("{}", e))?;
        Ok(StarMapMetaDto::from(sm))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NOT_FOUND", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// The caller must ensure `starmap_id_ptr` points to a valid, null-terminated C string.
/// Passing a null pointer or an invalid pointer is undefined behavior.
pub unsafe extern "C" fn writer_core_flush_starmap_store(
    starmap_id_ptr: *const c_char,
) -> *mut c_char {
    let starmap_id = match c_str_to_rust(starmap_id_ptr) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid starmap_id: error {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        svc.flush_starmap_store(starmap_id)
            .map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_ERROR", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// The caller must ensure `starmap_id_ptr` points to a valid, null-terminated C string.
/// Passing a null pointer or an invalid pointer is undefined behavior.
pub unsafe extern "C" fn writer_core_close_starmap_store(
    starmap_id_ptr: *const c_char,
) -> *mut c_char {
    let starmap_id = match c_str_to_rust(starmap_id_ptr) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid starmap_id: error {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        svc.close_starmap_store(starmap_id)
            .map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_ERROR", &e),
    }
}

#[no_mangle]
/// # Safety
///
/// This function does not take any pointer arguments, so there are no additional
/// safety requirements beyond those inherent to FFI boundary calls.
pub unsafe extern "C" fn writer_core_flush_all_starmap_stores() -> *mut c_char {
    match with_app_service(|svc| {
        svc.flush_all_starmap_stores()
            .map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_ERROR", &e),
    }
}
