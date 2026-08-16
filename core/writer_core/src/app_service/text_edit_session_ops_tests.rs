use crate::api::EditorEditOutcomeDto;
use crate::WriterAppService;

fn make_service() -> (WriterAppService, tempfile::TempDir) {
    let dir = tempfile::TempDir::new().unwrap();
    let svc = WriterAppService::new(
        dir.path().to_string_lossy().to_string(),
        dir.path().join("projects").to_string_lossy().to_string(),
    );
    (svc, dir)
}

/// #629 评论5306513458 问题2：`text_edit_session_update_composition` 成功后必须把
/// Core 真实的 composition_session（session_id / base_revision / generation）写回 DTO，
/// 不能只返回 `result.into()` 让 composition_session 保持 None。
#[test]
fn update_composition_writes_back_composition_session_dto() {
    let (svc, _dir) = make_service();
    let session_id = svc
        .text_edit_session_create("test".to_string(), String::new(), 0, 0)
        .expect("session created");

    // begin_composition 已正确写回 composition_session，作为 update 的输入基准。
    let begin = svc.text_edit_session_begin_composition(session_id, 0, 0, 0);
    assert_eq!(begin.outcome, EditorEditOutcomeDto::Applied);
    let begin_cs = begin
        .composition_session
        .expect("begin_composition writes composition_session");
    assert!(begin_cs.session_id > 0);

    // update_composition 成功后 DTO 必须带 Core 真实的 composition_session。
    let upd1 = svc.text_edit_session_update_composition(
        session_id,
        begin_cs.session_id,
        begin_cs.generation,
        "你".to_string(),
        1, // UTF-16 cursor offset
        begin.new_revision,
    );
    assert_eq!(upd1.outcome, EditorEditOutcomeDto::Applied);
    let upd1_cs = upd1
        .composition_session
        .expect("update_composition writes composition_session");
    assert_eq!(upd1_cs.session_id, begin_cs.session_id);
    assert_eq!(upd1_cs.base_revision, begin_cs.base_revision);
    assert!(
        upd1_cs.generation > begin_cs.generation,
        "generation should advance after update"
    );

    // 多次 update：generation 继续由 Core 裁判递增。
    let upd2 = svc.text_edit_session_update_composition(
        session_id,
        upd1_cs.session_id,
        upd1_cs.generation,
        "你好".to_string(),
        2, // UTF-16 cursor offset
        upd1.new_revision,
    );
    assert_eq!(upd2.outcome, EditorEditOutcomeDto::Applied);
    let upd2_cs = upd2
        .composition_session
        .expect("second update_composition writes composition_session");
    assert!(
        upd2_cs.generation > upd1_cs.generation,
        "generation should keep advancing across updates"
    );
}

/// update_composition 失败（stale generation）时不应写回 composition_session，
/// 与 begin_composition 的 outcome 守卫一致。
#[test]
fn update_composition_skips_writeback_on_stale_outcome() {
    let (svc, _dir) = make_service();
    let session_id = svc
        .text_edit_session_create("test".to_string(), String::new(), 0, 0)
        .expect("session created");
    let begin = svc.text_edit_session_begin_composition(session_id, 0, 0, 0);
    let begin_cs = begin
        .composition_session
        .expect("begin writes composition_session");

    // 用错误的 generation 触发 StaleRevision。
    let stale = svc.text_edit_session_update_composition(
        session_id,
        begin_cs.session_id,
        begin_cs.generation.saturating_add(999),
        "你".to_string(),
        1,
        begin.new_revision,
    );
    assert_eq!(stale.outcome, EditorEditOutcomeDto::StaleRevision);
    assert!(
        stale.composition_session.is_none(),
        "stale outcome must not write composition_session"
    );
}

/// #629 评论6 Part B：begin/update_composition 成功后必须把 Core 真实的 composition
/// 完整状态（preedit_text + replace range + cursor）写回 DTO.composition，
/// 平台端据此构造临时显示文本和下划线。snapshot 在 composition 活跃时也返回 composition。
#[test]
fn composition_state_is_exposed_in_edit_result_and_snapshot() {
    let (svc, _dir) = make_service();
    let session_id = svc
        .text_edit_session_create("test".to_string(), "abc".to_string(), 0, 0)
        .expect("session created");

    // begin_composition replace range [1, 2)（"b"）。
    let begin = svc.text_edit_session_begin_composition(session_id, 1, 2, 0);
    assert_eq!(begin.outcome, EditorEditOutcomeDto::Applied);
    let begin_comp = begin
        .composition
        .expect("begin_composition writes composition state");
    assert_eq!(begin_comp.replace_byte_start, 1);
    assert_eq!(begin_comp.replace_byte_end_exclusive, 2);
    assert!(begin_comp.preedit_text.is_empty());
    assert_eq!(begin_comp.preedit_cursor_utf16, 0);

    // update_composition 写入 preedit_text "你"。
    let upd = svc.text_edit_session_update_composition(
        session_id,
        begin_comp.session_id,
        begin_comp.generation,
        "你".to_string(),
        1,
        begin.new_revision,
    );
    assert_eq!(upd.outcome, EditorEditOutcomeDto::Applied);
    let upd_comp = upd
        .composition
        .expect("update_composition writes composition state");
    assert_eq!(upd_comp.replace_byte_start, 1);
    assert_eq!(upd_comp.replace_byte_end_exclusive, 2);
    assert_eq!(upd_comp.preedit_text, "你");
    assert_eq!(upd_comp.preedit_cursor_utf16, 1);

    // snapshot 在 composition 活跃时也返回 composition。
    let snap = svc.text_edit_session_snapshot(session_id);
    let snap_comp = snap
        .composition
        .expect("snapshot returns composition when active");
    assert_eq!(snap_comp.preedit_text, "你");
    assert_eq!(snap_comp.replace_byte_start, 1);
    assert_eq!(snap_comp.replace_byte_end_exclusive, 2);
    // 保存正文仍只取 committed text，不把 preedit 写进文件。
    assert_eq!(snap.text, "abc");

    // finish_composition 后 composition 为 None。
    let finish = svc.text_edit_session_finish_composition(
        session_id,
        upd_comp.session_id,
        upd_comp.generation,
        upd.new_revision,
    );
    assert_eq!(finish.outcome, EditorEditOutcomeDto::Applied);
    assert!(
        finish.composition.is_none(),
        "finish_composition clears composition"
    );
    let snap2 = svc.text_edit_session_snapshot(session_id);
    assert!(
        snap2.composition.is_none(),
        "snapshot returns no composition after finish"
    );
}

/// #629 评论7 第1项：composition preedit cursor 现在用 `Utf16CodeUnitOffset` 强类型，
/// 按 UTF-16 code unit 长度校验。中文 "你" UTF-16 len=1，cursor=1 合法。
#[test]
fn composition_update_chinese_preedit_cursor_utf16_valid() {
    let (svc, _dir) = make_service();
    let session_id = svc
        .text_edit_session_create("test".to_string(), "abc".to_string(), 3, 3)
        .expect("session created");
    let begin = svc.text_edit_session_begin_composition(session_id, 3, 3, 0);
    assert_eq!(begin.outcome, EditorEditOutcomeDto::Applied);
    let begin_cs = begin.composition.expect("begin writes composition");
    // 中文 "你" UTF-16 len=1，cursor=1 合法
    let upd = svc.text_edit_session_update_composition(
        session_id,
        begin_cs.session_id,
        begin_cs.generation,
        "你".to_string(),
        1,
        begin.new_revision,
    );
    assert_eq!(upd.outcome, EditorEditOutcomeDto::Applied);
    let upd_comp = upd.composition.expect("update writes composition");
    assert_eq!(upd_comp.preedit_text, "你");
    assert_eq!(upd_comp.preedit_cursor_utf16, 1);
}

/// #629 评论7 第1项：中文 "你" UTF-16 len=1，cursor=3 越界 → InvalidOffset。
#[test]
fn composition_update_chinese_preedit_cursor_utf16_beyond_end_rejected() {
    let (svc, _dir) = make_service();
    let session_id = svc
        .text_edit_session_create("test".to_string(), "abc".to_string(), 3, 3)
        .expect("session created");
    let begin = svc.text_edit_session_begin_composition(session_id, 3, 3, 0);
    assert_eq!(begin.outcome, EditorEditOutcomeDto::Applied);
    let begin_cs = begin.composition.expect("begin writes composition");
    // cursor=3 > utf16_len=1 → InvalidOffset
    let upd = svc.text_edit_session_update_composition(
        session_id,
        begin_cs.session_id,
        begin_cs.generation,
        "你".to_string(),
        3,
        begin.new_revision,
    );
    assert_eq!(upd.outcome, EditorEditOutcomeDto::InvalidOffset);
}

/// #629 评论7 第1项：emoji "👨‍👩‍👧"（ZWJ 序列）UTF-16 len=8，cursor=4 合法。
#[test]
fn composition_update_emoji_zwj_preedit_cursor_utf16_valid() {
    let (svc, _dir) = make_service();
    let session_id = svc
        .text_edit_session_create("test".to_string(), "abc".to_string(), 3, 3)
        .expect("session created");
    let begin = svc.text_edit_session_begin_composition(session_id, 3, 3, 0);
    assert_eq!(begin.outcome, EditorEditOutcomeDto::Applied);
    let begin_cs = begin.composition.expect("begin writes composition");
    // family emoji with ZWJ: 👨(2) + ZWJ(1) + 👩(2) + ZWJ(1) + 👧(2) = 8 UTF-16 code units
    let preedit = "👨‍👩‍👧";
    let utf16_len: usize = preedit.chars().map(|c| c.len_utf16()).sum();
    assert_eq!(utf16_len, 8);
    let upd = svc.text_edit_session_update_composition(
        session_id,
        begin_cs.session_id,
        begin_cs.generation,
        preedit.to_string(),
        4,
        begin.new_revision,
    );
    assert_eq!(upd.outcome, EditorEditOutcomeDto::Applied);
    let upd_comp = upd.composition.expect("update writes composition");
    assert_eq!(upd_comp.preedit_text, preedit);
    assert_eq!(upd_comp.preedit_cursor_utf16, 4);
}
