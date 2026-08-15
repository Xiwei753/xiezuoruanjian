use super::parse_cause;
use crate::api::EditorTransactionCauseDto;

#[test]
fn test_parse_cause_typing() {
    assert!(matches!(
        parse_cause("Typing"),
        Ok(EditorTransactionCauseDto::Typing)
    ));
}

#[test]
fn test_parse_cause_delete() {
    assert!(matches!(
        parse_cause("Delete"),
        Ok(EditorTransactionCauseDto::Delete)
    ));
}

#[test]
fn test_parse_cause_ime_composition() {
    assert!(matches!(
        parse_cause("ImeComposition"),
        Ok(EditorTransactionCauseDto::ImeComposition)
    ));
}

#[test]
fn test_parse_cause_typing_commit() {
    assert!(matches!(
        parse_cause("TypingCommit"),
        Ok(EditorTransactionCauseDto::TypingCommit)
    ));
}

#[test]
fn test_parse_cause_paste() {
    assert!(matches!(
        parse_cause("Paste"),
        Ok(EditorTransactionCauseDto::Paste)
    ));
}

#[test]
fn test_parse_cause_undo() {
    assert!(matches!(
        parse_cause("Undo"),
        Ok(EditorTransactionCauseDto::Undo)
    ));
}

#[test]
fn test_parse_cause_redo() {
    assert!(matches!(
        parse_cause("Redo"),
        Ok(EditorTransactionCauseDto::Redo)
    ));
}

#[test]
fn test_parse_cause_load() {
    assert!(matches!(
        parse_cause("Load"),
        Ok(EditorTransactionCauseDto::Load)
    ));
}

#[test]
fn test_parse_cause_format() {
    assert!(matches!(
        parse_cause("Format"),
        Ok(EditorTransactionCauseDto::Format)
    ));
}

#[test]
fn test_parse_cause_programmatic() {
    assert!(matches!(
        parse_cause("Programmatic"),
        Ok(EditorTransactionCauseDto::Programmatic)
    ));
}

#[test]
fn test_parse_cause_unknown() {
    assert!(parse_cause("Unknown").is_err());
}

#[test]
fn test_parse_cause_foo() {
    assert!(parse_cause("foo").is_err());
}

#[test]
fn test_parse_cause_empty() {
    assert!(parse_cause("").is_err());
}

// ── FFI 端到端集成测试 ──
// 验证 editor_session C ABI 全生命周期，闭环 Issue #629 评论第 4 节
// "接通自研写作区到 Rust TextEditSession"。
// byte offset 始终是 UTF-8 byte offset（评论明确要求），用非 ASCII 文本验证。

use std::ffi::CString;
use std::os::raw::c_char;
use std::sync::OnceLock;
use tempfile::tempdir;

/// 持有测试用 TempDir 到进程结束，避免其析构删除目录而全局 Core 仍引用。
/// 不是资源泄漏——由 OnceLock 合法拥有。
static TEST_TEMP_DIR: OnceLock<Option<tempfile::TempDir>> = OnceLock::new();

/// 调用 C ABI 后把返回的 C string 解析为 JSON Value 并释放。
///
/// # Safety
/// `ptr` 必须是 `writer_core_*` 返回的、由 Rust 分配的 C string。
unsafe fn call_ffi(ptr: *mut c_char) -> serde_json::Value {
    assert!(!ptr.is_null(), "FFI 返回空指针");
    // SAFETY: ptr 非空且由调用方保证是 writer_core_* 返回的 Rust 分配 C string。
    let s = unsafe { std::ffi::CStr::from_ptr(ptr) }
        .to_str()
        .expect("FFI 返回非 UTF-8");
    let v: serde_json::Value = serde_json::from_str(s).expect("FFI 返回非 JSON");
    // SAFETY: ptr 由 ok_json/err_json 中 CString::into_raw 分配，调用方保证不重复释放。
    unsafe {
        crate::ffi::writer_core_free_string(ptr);
    }
    v
}

/// 初始化全局 Core（幂等：OnceLock 保证 APP_SERVICE 只建一次）。
fn ensure_core_init() {
    let dir = tempdir().expect("无法创建临时目录");
    let path = CString::new(dir.path().to_str().unwrap()).unwrap();
    // SAFETY: path 是有效的 NUL-terminated UTF-8 C string。
    let rc = unsafe { crate::ffi::writer_core_init(path.as_ptr()) };
    assert_eq!(rc, 0, "writer_core_init 失败");
    // 持有 TempDir 到进程结束，避免其析构删除目录而全局 Core 仍引用。
    let _ = TEST_TEMP_DIR.set(Some(dir));
}

/// 断言 JSON 是成功信封，返回 `data` 引用。
fn assert_success(v: &serde_json::Value) -> &serde_json::Value {
    assert_eq!(v["success"], true, "期望成功信封，实际: {v}");
    &v["data"]
}

/// 调用 snapshot 并返回 text 字段。
fn snapshot_text(session_id: u64) -> String {
    // SAFETY: session_id 有效。
    let v = unsafe { call_ffi(super::writer_core_editor_session_snapshot(session_id)) };
    assert_success(&v)["text"]
        .as_str()
        .expect("snapshot text 缺失")
        .to_string()
}

#[test]
fn test_ffi_editor_session_lifecycle() {
    ensure_core_init();
    let target = CString::new("ffi_test_lifecycle").unwrap();
    let initial = CString::new("Hello").unwrap();
    // SAFETY: C string 参数有效。
    let v = unsafe {
        call_ffi(super::writer_core_editor_session_create(
            target.as_ptr(),
            initial.as_ptr(),
            0,
            0,
        ))
    };
    let session_id: u64 = assert_success(&v).as_u64().expect("create 未返回 u64");

    // insert " World" at byte offset 5 → "Hello World"
    let text = CString::new(" World").unwrap();
    let cause = CString::new("Typing").unwrap();
    // SAFETY: C string 参数由 CString 创建，session_id 由 create 返回，均有效。
    let v = unsafe {
        call_ffi(super::writer_core_editor_session_insert(
            session_id,
            5,
            text.as_ptr(),
            cause.as_ptr(),
            0,
        ))
    };
    let data = assert_success(&v);
    assert_eq!(data["outcome"], "applied");
    let rev1 = data["newRevision"].as_u64().expect("newRevision 缺失");
    assert_eq!(snapshot_text(session_id), "Hello World");

    // set_selection [0, 5]
    // SAFETY: C string 参数由 CString 创建，session_id 由 create 返回，均有效。
    let v = unsafe {
        call_ffi(super::writer_core_editor_session_set_selection(
            session_id, 0, 5, rev1,
        ))
    };
    let data = assert_success(&v);
    assert_eq!(data["newSelectionStart"], 0);
    assert_eq!(data["newSelectionEnd"], 5);
    let rev2 = data["newRevision"].as_u64().unwrap();

    // delete [0, 5] → "World"
    let cause = CString::new("Delete").unwrap();
    // SAFETY: C string 参数由 CString 创建，session_id 由 create 返回，均有效。
    let v = unsafe {
        call_ffi(super::writer_core_editor_session_delete(
            session_id,
            0,
            5,
            cause.as_ptr(),
            rev2,
        ))
    };
    let data = assert_success(&v);
    assert_eq!(data["outcome"], "applied");
    let rev3 = data["newRevision"].as_u64().unwrap();
    assert_eq!(snapshot_text(session_id), " World");

    // undo → "Hello World"
    // SAFETY: session_id 由 create 返回，rev3 由前序操作返回，均有效。
    let v = unsafe { call_ffi(super::writer_core_editor_session_undo(session_id, rev3)) };
    let data = assert_success(&v);
    assert_eq!(data["outcome"], "applied");
    let rev4 = data["newRevision"].as_u64().unwrap();
    assert_eq!(snapshot_text(session_id), "Hello World");

    // redo → "World"
    // SAFETY: session_id 由 create 返回，rev4 由前序操作返回，均有效。
    let v = unsafe { call_ffi(super::writer_core_editor_session_redo(session_id, rev4)) };
    assert_success(&v);
    assert_eq!(snapshot_text(session_id), " World");

    // close
    // SAFETY: C string 参数由 CString 创建，session_id 由 create 返回，均有效。
    let v = unsafe { call_ffi(super::writer_core_editor_session_close(session_id)) };
    assert_success(&v);
}

/// 验证 byte offset 始终是 UTF-8 byte offset（评论第 4 节核心要求）。
#[test]
fn test_ffi_editor_session_utf8_byte_offset() {
    ensure_core_init();
    // "你好" = 你(3 bytes) + 好(3 bytes) = 6 bytes
    let target = CString::new("ffi_test_utf8").unwrap();
    let initial = CString::new("你好").unwrap();
    // SAFETY: C string 参数由 CString 创建，session_id 由 create 返回，均有效。
    let v = unsafe {
        call_ffi(super::writer_core_editor_session_create(
            target.as_ptr(),
            initial.as_ptr(),
            0,
            0,
        ))
    };
    let session_id: u64 = assert_success(&v).as_u64().unwrap();

    // insert "X" at byte offset 3（你 之后）→ "你X好"
    let text = CString::new("X").unwrap();
    let cause = CString::new("Typing").unwrap();
    // SAFETY: C string 参数由 CString 创建，session_id 由 create 返回，均有效。
    let v = unsafe {
        call_ffi(super::writer_core_editor_session_insert(
            session_id,
            3,
            text.as_ptr(),
            cause.as_ptr(),
            0,
        ))
    };
    let data = assert_success(&v);
    assert_eq!(data["outcome"], "applied");
    assert_eq!(
        snapshot_text(session_id),
        "你X好",
        "byte offset 3 应在 你 之后"
    );
    let rev = data["newRevision"].as_u64().unwrap();

    // delete [0, 3)（移除 你）→ "X好"
    let cause = CString::new("Delete").unwrap();
    // SAFETY: C string 参数由 CString 创建，session_id 由 create 返回，均有效。
    let v = unsafe {
        call_ffi(super::writer_core_editor_session_delete(
            session_id,
            0,
            3,
            cause.as_ptr(),
            rev,
        ))
    };
    assert_success(&v);
    assert_eq!(
        snapshot_text(session_id),
        "X好",
        "删除 byte [0,3) 应移除 你"
    );

    // close
    // SAFETY: C string 参数由 CString 创建，session_id 由 create 返回，均有效。
    let v = unsafe { call_ffi(super::writer_core_editor_session_close(session_id)) };
    assert_success(&v);
}

/// 验证 IME composition 全生命周期（begin → update → finish）。
#[test]
fn test_ffi_editor_session_composition_lifecycle() {
    ensure_core_init();
    let target = CString::new("ffi_test_composition").unwrap();
    let initial = CString::new("").unwrap();
    // SAFETY: C string 参数由 CString 创建，session_id 由 create 返回，均有效。
    let v = unsafe {
        call_ffi(super::writer_core_editor_session_create(
            target.as_ptr(),
            initial.as_ptr(),
            0,
            0,
        ))
    };
    let session_id: u64 = assert_success(&v).as_u64().unwrap();

    // 取初始 revision
    // SAFETY: C string 参数由 CString 创建，session_id 由 create 返回，均有效。
    let v = unsafe { call_ffi(super::writer_core_editor_session_snapshot(session_id)) };
    let rev0 = assert_success(&v)["revision"].as_u64().unwrap();

    // begin composition [0, 0]
    // SAFETY: C string 参数由 CString 创建，session_id 由 create 返回，均有效。
    let v = unsafe {
        call_ffi(super::writer_core_editor_session_begin_composition(
            session_id, 0, 0, rev0,
        ))
    };
    let data = assert_success(&v);
    assert_eq!(data["outcome"], "applied");
    let comp = &data["compositionSession"];
    assert!(
        comp.is_object(),
        "begin_composition 应返回 compositionSession"
    );
    let comp_session_id = comp["sessionId"].as_u64().unwrap();
    let comp_generation = comp["generation"].as_u64().unwrap();
    let rev1 = data["newRevision"].as_u64().unwrap();

    // update composition with preedit "ni"（2 字节，cursor offset 2）
    let preedit = CString::new("ni").unwrap();
    // SAFETY: C string 参数由 CString 创建，session_id 由 create 返回，均有效。
    let v = unsafe {
        call_ffi(super::writer_core_editor_session_update_composition(
            session_id,
            comp_session_id,
            comp_generation,
            preedit.as_ptr(),
            2,
            rev1,
        ))
    };
    let data = assert_success(&v);
    assert_eq!(data["outcome"], "applied");
    let rev2 = data["newRevision"].as_u64().unwrap();

    // finish composition → preedit 文本提交为正文
    // update_composition 递增了 generation（0→1），finish 需用最新 generation。
    // SAFETY: C string 参数由 CString 创建，session_id 由 create 返回，均有效。
    let v = unsafe {
        call_ffi(super::writer_core_editor_session_finish_composition(
            session_id,
            comp_session_id,
            comp_generation + 1,
            rev2,
        ))
    };
    let data = assert_success(&v);
    assert_eq!(data["outcome"], "applied");
    assert_eq!(
        snapshot_text(session_id),
        "ni",
        "finish composition 应提交 preedit 文本"
    );

    // close
    // SAFETY: C string 参数由 CString 创建，session_id 由 create 返回，均有效。
    let v = unsafe { call_ffi(super::writer_core_editor_session_close(session_id)) };
    assert_success(&v);
}
