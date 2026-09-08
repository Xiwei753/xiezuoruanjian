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

// ── #629 R8: composition grapheme semantic operation FFI tests ──

/// 辅助：创建 session + begin composition + update preedit，返回 (session_id, comp_session_id, comp_generation, revision)
#[allow(clippy::cast_possible_truncation)]
fn setup_composition(target: &str, initial: &str, preedit: &str) -> (u64, u64, u64, u64) {
    let target_cstr = CString::new(target).unwrap();
    let initial_cstr = CString::new(initial).unwrap();
    // SAFETY: C string 参数有效。
    let v = unsafe {
        call_ffi(super::writer_core_editor_session_create(
            target_cstr.as_ptr(),
            initial_cstr.as_ptr(),
            0,
            0,
        ))
    };
    let session_id: u64 = assert_success(&v).as_u64().unwrap();
    let initial_len = initial.len() as u32;
    // SAFETY: session_id 有效。
    let v = unsafe {
        call_ffi(super::writer_core_editor_session_begin_composition(
            session_id,
            initial_len,
            initial_len,
            0,
        ))
    };
    let data = assert_success(&v);
    let comp = &data["compositionSession"];
    let comp_session_id = comp["sessionId"].as_u64().unwrap();
    let comp_generation = comp["generation"].as_u64().unwrap();
    let rev1 = data["newRevision"].as_u64().unwrap();
    let preedit_cstr = CString::new(preedit).unwrap();
    // preedit cursor 是 UTF-16 code unit offset（不是 UTF-8 byte len）。
    let preedit_utf16_len: u32 = preedit.chars().map(|c| c.len_utf16() as u32).sum();
    // SAFETY: C string 参数有效。
    let v = unsafe {
        call_ffi(super::writer_core_editor_session_update_composition(
            session_id,
            comp_session_id,
            comp_generation,
            preedit_cstr.as_ptr(),
            preedit_utf16_len,
            rev1,
        ))
    };
    let data = assert_success(&v);
    let rev2 = data["newRevision"].as_u64().unwrap();
    let new_gen = data["compositionSession"]["generation"].as_u64().unwrap();
    (session_id, comp_session_id, new_gen, rev2)
}

#[allow(clippy::cast_possible_truncation)]
fn snapshot_composition(session_id: u64) -> (String, u32, u64) {
    // SAFETY: session_id 有效。
    let v = unsafe { call_ffi(super::writer_core_editor_session_snapshot(session_id)) };
    let data = assert_success(&v);
    let comp = &data["composition"];
    (
        comp["preeditText"].as_str().unwrap().to_string(),
        comp["preeditCursorUtf16"].as_u64().unwrap() as u32,
        comp["generation"].as_u64().unwrap(),
    )
}

#[test]
fn test_ffi_composition_move_grapheme_left_ascii() {
    ensure_core_init();
    let (sid, comp_sid, comp_gen, rev) = setup_composition("ffi_comp_move_l", "", "hello");
    // SAFETY: C string 参数有效，session_id 由 setup_composition 返回。
    let v = unsafe {
        call_ffi(
            super::writer_core_editor_session_composition_move_grapheme_left(
                sid, comp_sid, comp_gen, rev,
            ),
        )
    };
    let data = assert_success(&v);
    assert_eq!(data["outcome"], "applied");
    assert_eq!(snapshot_text(sid), "");
    let (preedit, cursor, _gen) = snapshot_composition(sid);
    assert_eq!(preedit, "hello");
    assert_eq!(cursor, 4, "move left: cursor 从 5 移到 4");
    // SAFETY: session_id 有效。
    let v = unsafe { call_ffi(super::writer_core_editor_session_close(sid)) };
    assert_success(&v);
}

#[test]
fn test_ffi_composition_move_grapheme_right_ascii() {
    ensure_core_init();
    let (sid, comp_sid, comp_gen, rev) = setup_composition("ffi_comp_move_r", "", "hello");
    // SAFETY: C string 参数有效，session_id 由 setup_composition 返回。
    let v = unsafe {
        call_ffi(
            super::writer_core_editor_session_composition_move_grapheme_left(
                sid, comp_sid, comp_gen, rev,
            ),
        )
    };
    let data = assert_success(&v);
    let rev2 = data["newRevision"].as_u64().unwrap();
    let gen2 = data["compositionSession"]["generation"].as_u64().unwrap();
    // SAFETY: C string 参数有效，session_id 由 setup_composition 返回。
    let v = unsafe {
        call_ffi(
            super::writer_core_editor_session_composition_move_grapheme_right(
                sid, comp_sid, gen2, rev2,
            ),
        )
    };
    let data = assert_success(&v);
    assert_eq!(data["outcome"], "applied");
    let (preedit, cursor, _gen) = snapshot_composition(sid);
    assert_eq!(preedit, "hello");
    assert_eq!(cursor, 5, "move right: cursor 回到 5");
    // SAFETY: session_id 有效。
    let v = unsafe { call_ffi(super::writer_core_editor_session_close(sid)) };
    assert_success(&v);
}

#[test]
fn test_ffi_composition_delete_grapheme_backward_ascii() {
    ensure_core_init();
    let (sid, comp_sid, comp_gen, rev) = setup_composition("ffi_comp_del_b", "", "hello");
    // SAFETY: C string 参数有效，session_id 由 setup_composition 返回。
    let v = unsafe {
        call_ffi(
            super::writer_core_editor_session_composition_delete_grapheme_backward(
                sid, comp_sid, comp_gen, rev,
            ),
        )
    };
    let data = assert_success(&v);
    assert_eq!(data["outcome"], "applied");
    assert_eq!(snapshot_text(sid), "");
    let (preedit, cursor, _gen) = snapshot_composition(sid);
    assert_eq!(preedit, "hell", "delete backward 应删除最后一个字符");
    assert_eq!(cursor, 4, "cursor 应回退");
    // SAFETY: session_id 有效。
    let v = unsafe { call_ffi(super::writer_core_editor_session_close(sid)) };
    assert_success(&v);
}

#[test]
fn test_ffi_composition_delete_grapheme_forward_ascii() {
    ensure_core_init();
    let (sid, comp_sid, comp_gen, rev) = setup_composition("ffi_comp_del_f", "", "hello");
    // SAFETY: C string 参数有效，session_id 由 setup_composition 返回。
    let v = unsafe {
        call_ffi(
            super::writer_core_editor_session_composition_move_grapheme_left(
                sid, comp_sid, comp_gen, rev,
            ),
        )
    };
    let data = assert_success(&v);
    let rev2 = data["newRevision"].as_u64().unwrap();
    let gen2 = data["compositionSession"]["generation"].as_u64().unwrap();
    // SAFETY: C string 参数有效，session_id 由 setup_composition 返回。
    let v = unsafe {
        call_ffi(
            super::writer_core_editor_session_composition_move_grapheme_left(
                sid, comp_sid, gen2, rev2,
            ),
        )
    };
    let data = assert_success(&v);
    let rev3 = data["newRevision"].as_u64().unwrap();
    let gen3 = data["compositionSession"]["generation"].as_u64().unwrap();
    // SAFETY: C string 参数有效，session_id 由 setup_composition 返回。
    let v = unsafe {
        call_ffi(
            super::writer_core_editor_session_composition_delete_grapheme_forward(
                sid, comp_sid, gen3, rev3,
            ),
        )
    };
    let data = assert_success(&v);
    assert_eq!(data["outcome"], "applied");
    let (preedit, cursor, _gen) = snapshot_composition(sid);
    assert_eq!(preedit, "helo", "delete forward 应删除位置 3 的字符");
    assert_eq!(cursor, 3, "cursor 应保持在 3");
    // SAFETY: session_id 有效。
    let v = unsafe { call_ffi(super::writer_core_editor_session_close(sid)) };
    assert_success(&v);
}

#[test]
fn test_ffi_composition_move_left_at_boundary_noop() {
    ensure_core_init();
    let (sid, comp_sid, comp_gen, rev) = setup_composition("ffi_comp_noop_l", "", "ab");
    // SAFETY: C string 参数有效，session_id 由 setup_composition 返回。
    let v = unsafe {
        call_ffi(
            super::writer_core_editor_session_composition_move_grapheme_left(
                sid, comp_sid, comp_gen, rev,
            ),
        )
    };
    let data = assert_success(&v);
    let rev2 = data["newRevision"].as_u64().unwrap();
    let gen2 = data["compositionSession"]["generation"].as_u64().unwrap();
    let (_, cursor2, _) = snapshot_composition(sid);
    assert_eq!(cursor2, 1, "move left from end of \"ab\" should go to 1");
    // SAFETY: C string 参数有效，session_id 由 setup_composition 返回。
    let v = unsafe {
        call_ffi(
            super::writer_core_editor_session_composition_move_grapheme_left(
                sid, comp_sid, gen2, rev2,
            ),
        )
    };
    assert_success(&v);
    let (preedit, cursor, _) = snapshot_composition(sid);
    assert_eq!(preedit, "ab");
    assert_eq!(cursor, 0, "second move left should reach boundary 0");
    // SAFETY: session_id 有效。
    let v = unsafe { call_ffi(super::writer_core_editor_session_close(sid)) };
    assert_success(&v);
}

#[test]
fn test_ffi_composition_move_right_at_boundary_noop() {
    ensure_core_init();
    let (sid, comp_sid, comp_gen, rev) = setup_composition("ffi_comp_noop_r", "", "ab");
    // SAFETY: C string 参数有效，session_id 由 setup_composition 返回。
    let v = unsafe {
        call_ffi(
            super::writer_core_editor_session_composition_move_grapheme_right(
                sid, comp_sid, comp_gen, rev,
            ),
        )
    };
    assert_success(&v);
    let (preedit, cursor, _) = snapshot_composition(sid);
    assert_eq!(preedit, "ab");
    assert_eq!(cursor, 2, "boundary no-op");
    // SAFETY: session_id 有效。
    let v = unsafe { call_ffi(super::writer_core_editor_session_close(sid)) };
    assert_success(&v);
}

#[test]
fn test_ffi_composition_committed_text_unchanged() {
    ensure_core_init();
    let (sid, comp_sid, comp_gen, rev) = setup_composition("ffi_comp_invar", "committed", "预输入");
    // SAFETY: C string 参数有效，session_id 由 setup_composition 返回。
    let v = unsafe {
        call_ffi(
            super::writer_core_editor_session_composition_move_grapheme_left(
                sid, comp_sid, comp_gen, rev,
            ),
        )
    };
    let move_data = assert_success(&v);
    assert_eq!(move_data["outcome"], "applied");
    assert_eq!(snapshot_text(sid), "committed", "move left 后正文不变");
    let (_, _, gen2) = snapshot_composition(sid);
    // SAFETY: C string 参数有效，session_id 由 setup_composition 返回。
    let v = unsafe {
        call_ffi(
            super::writer_core_editor_session_composition_delete_grapheme_backward(
                sid, comp_sid, gen2, rev,
            ),
        )
    };
    assert_success(&v);
    assert_eq!(
        snapshot_text(sid),
        "committed",
        "delete backward 后正文不变"
    );
    // SAFETY: session_id 有效。
    let v = unsafe { call_ffi(super::writer_core_editor_session_close(sid)) };
    assert_success(&v);
}

#[test]
fn test_ffi_composition_stale_session_rejected() {
    ensure_core_init();
    let (sid, _comp_sid, _comp_gen, rev) = setup_composition("ffi_comp_stale", "", "test");
    // SAFETY: C string 参数有效，session_id 由 setup_composition 返回。
    let v = unsafe {
        call_ffi(
            super::writer_core_editor_session_composition_move_grapheme_left(sid, 999999, 0, rev),
        )
    };
    let data = assert_success(&v);
    assert_eq!(
        data["outcome"], "staleRevision",
        "错误 session_id 应返回 stale"
    );
    // SAFETY: session_id 有效。
    let v = unsafe { call_ffi(super::writer_core_editor_session_close(sid)) };
    assert_success(&v);
}

#[test]
fn test_ffi_composition_generation_increments() {
    ensure_core_init();
    let (sid, comp_sid, comp_gen, rev) = setup_composition("ffi_comp_gen", "", "abc");
    // SAFETY: C string 参数有效，session_id 由 setup_composition 返回。
    let v = unsafe {
        call_ffi(
            super::writer_core_editor_session_composition_move_grapheme_left(
                sid, comp_sid, comp_gen, rev,
            ),
        )
    };
    let data = assert_success(&v);
    let gen_after_move = data["compositionSession"]["generation"].as_u64().unwrap();
    assert_eq!(gen_after_move, comp_gen + 1, "move 后 generation 应递增");
    // SAFETY: session_id 有效。
    let v = unsafe { call_ffi(super::writer_core_editor_session_close(sid)) };
    assert_success(&v);
}

#[test]
fn test_ffi_composition_combining_chars() {
    // 分解序列 e + U+0301：UTF-16 2 units、2 个 code point 是同一个 grapheme。
    ensure_core_init();
    let (sid, comp_sid, comp_gen, rev) = setup_composition("ffi_comp_combining", "", "e\u{301}");
    // SAFETY: C string 参数有效，session_id 由 setup_composition 返回。
    let v = unsafe {
        call_ffi(
            super::writer_core_editor_session_composition_move_grapheme_left(
                sid, comp_sid, comp_gen, rev,
            ),
        )
    };
    let data = assert_success(&v);
    assert_eq!(data["outcome"], "applied");
    let (preedit, cursor, _gen) = snapshot_composition(sid);
    assert_eq!(preedit, "e\u{301}", "move 不改 preedit 文本");
    assert_eq!(cursor, 0, "combining move left: 应跳过整个 grapheme 到 0");

    // 重新放回 cursor 到末尾，delete backward 应一次删掉整个组合。
    let rev2 = data["newRevision"].as_u64().unwrap();
    let gen2 = data["compositionSession"]["generation"].as_u64().unwrap();
    // SAFETY: C string 参数有效，session_id 由 setup_composition 返回。
    let v = unsafe {
        call_ffi(super::writer_core_editor_session_update_composition(
            sid,
            comp_sid,
            gen2,
            CString::new("e\u{301}").unwrap().as_ptr(),
            2,
            rev2,
        ))
    };
    let data = assert_success(&v);
    let rev3 = data["newRevision"].as_u64().unwrap();
    let gen3 = data["compositionSession"]["generation"].as_u64().unwrap();
    // SAFETY: C string 参数有效，session_id 由 setup_composition 返回。
    let v = unsafe {
        call_ffi(
            super::writer_core_editor_session_composition_delete_grapheme_backward(
                sid, comp_sid, gen3, rev3,
            ),
        )
    };
    assert_success(&v);
    let (preedit, cursor, _) = snapshot_composition(sid);
    assert_eq!(
        preedit, "",
        "combining delete backward: 应删除整个 grapheme"
    );
    assert_eq!(cursor, 0, "cursor 应回到 0");
    // SAFETY: session_id 有效。
    let v = unsafe { call_ffi(super::writer_core_editor_session_close(sid)) };
    assert_success(&v);
}

#[test]
fn test_ffi_composition_zwj_emoji() {
    // ZWJ family emoji（👨‍👩‍👧‍👦）整体是一个 grapheme：move left 一次到 0，
    // delete backward 一次清空，正文不变。
    ensure_core_init();
    let (sid, comp_sid, comp_gen, rev) = setup_composition("ffi_comp_zwj", "", "👨‍👩‍👧‍👦");
    // setup_composition 已把 preedit cursor 放在末尾（UTF-16 11）。
    // SAFETY: C string 参数有效，session_id 由 setup_composition 返回。
    let v = unsafe {
        call_ffi(
            super::writer_core_editor_session_composition_move_grapheme_left(
                sid, comp_sid, comp_gen, rev,
            ),
        )
    };
    let data = assert_success(&v);
    let (preedit, cursor, _) = snapshot_composition(sid);
    assert_eq!(preedit, "👨‍👩‍👧‍👦");
    assert_eq!(cursor, 0, "ZWJ emoji move left: cursor 应一步到 0");

    // 用 move 后的 generation 把 cursor 放回末尾，再 delete backward。
    let gen_after_move = data["compositionSession"]["generation"].as_u64().unwrap();
    let rev_after_move = data["newRevision"].as_u64().unwrap();
    // SAFETY: C string 参数有效，session_id 由 setup_composition 返回。
    let v = unsafe {
        call_ffi(super::writer_core_editor_session_update_composition(
            sid,
            comp_sid,
            gen_after_move,
            CString::new("👨‍👩‍👧‍👦").unwrap().as_ptr(),
            11,
            rev_after_move,
        ))
    };
    let data = assert_success(&v);
    let rev3 = data["newRevision"].as_u64().unwrap();
    let gen3 = data["compositionSession"]["generation"].as_u64().unwrap();
    // SAFETY: C string 参数有效，session_id 由 setup_composition 返回。
    let v = unsafe {
        call_ffi(
            super::writer_core_editor_session_composition_delete_grapheme_backward(
                sid, comp_sid, gen3, rev3,
            ),
        )
    };
    assert_success(&v);
    let (preedit, cursor, _) = snapshot_composition(sid);
    assert_eq!(preedit, "", "ZWJ emoji delete backward: preedit 应清空");
    assert_eq!(cursor, 0);
    // SAFETY: session_id 有效。
    let v = unsafe { call_ffi(super::writer_core_editor_session_close(sid)) };
    assert_success(&v);
}
