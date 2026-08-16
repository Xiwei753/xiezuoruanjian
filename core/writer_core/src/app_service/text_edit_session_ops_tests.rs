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

/// #629 评论8 第4项：所有 Core 命令统一 composition 回填出口。
/// setSelection（outcome NoChange）在 composition 活跃期间必须回填 composition_session/composition，
/// 与 kernel 真实状态一致——这是"Core 有 composition、平台 DTO 却是 null"的直接修复场景。
/// finish 后再 setSelection：composition 已清，DTO 保持 None。
#[allow(clippy::cast_possible_truncation)]
#[test]
fn set_selection_during_composition_backfills_dto() {
    let (svc, _dir) = make_service();
    let session_id = svc
        .text_edit_session_create("test".to_string(), "abc".to_string(), 3, 0)
        .expect("session created");

    let begin = svc.text_edit_session_begin_composition(session_id, 1, 2, 0);
    assert_eq!(begin.outcome, EditorEditOutcomeDto::Applied);
    let begin_comp = begin.composition.expect("begin writes composition");
    let upd = svc.text_edit_session_update_composition(
        session_id,
        begin_comp.session_id,
        begin_comp.generation,
        "你".to_string(),
        1,
        begin.new_revision,
    );
    assert_eq!(upd.outcome, EditorEditOutcomeDto::Applied);
    let upd_cs = upd
        .composition_session
        .expect("update writes composition_session");

    // setSelection 不结束 composition（kernel 不清 composition）：DTO 必须回填当前 composition。
    // 注：ops 层用 From<EditorEditResult> 转 DTO，outcome 恒为 Applied（现有约定），
    // 关键断言是 composition_session/composition 与 kernel 真实状态一致。
    let sel = svc.text_edit_session_set_selection(session_id, 0, 0, upd.new_revision);
    assert_eq!(sel.outcome, EditorEditOutcomeDto::Applied);
    let sel_cs = sel
        .composition_session
        .expect("setSelection backfills composition_session");
    assert_eq!(sel_cs.session_id, upd_cs.session_id);
    assert_eq!(sel_cs.base_revision, upd_cs.base_revision);
    assert_eq!(sel_cs.generation, upd_cs.generation);
    let sel_comp = sel
        .composition
        .expect("setSelection backfills composition state");
    assert_eq!(sel_comp.preedit_text, "你");
    assert_eq!(sel_comp.replace_byte_start, 1);
    assert_eq!(sel_comp.replace_byte_end_exclusive, 2);
    assert_eq!(sel_comp.preedit_cursor_utf16, 1);

    // finish 后再 setSelection：composition 已清，DTO 保持 None（不残留旧 composition）。
    let finish = svc.text_edit_session_finish_composition(
        session_id,
        upd_cs.session_id,
        upd_cs.generation,
        upd.new_revision,
    );
    assert_eq!(finish.outcome, EditorEditOutcomeDto::Applied);
    assert!(finish.composition_session.is_none());
    assert!(finish.composition.is_none());
    let sel2 = svc.text_edit_session_set_selection(session_id, 0, 0, finish.new_revision);
    assert_eq!(sel2.outcome, EditorEditOutcomeDto::Applied);
    assert!(
        sel2.composition_session.is_none(),
        "finish 后 setSelection 不残留 composition_session"
    );
    assert!(
        sel2.composition.is_none(),
        "finish 后 setSelection 不残留 composition"
    );
}

/// #629 评论8 第4项：text-modifying 命令（insert/delete/undo/redo/commitText/
/// deleteSurrounding/replaceAll/insertLineBreak）成功时 kernel 会清掉 composition——
/// DTO 必须同样反映"无活跃 composition"（None），与 snapshot() 一致，平台端不会
/// 看到 Core 有 composition、DTO 却是 null 的脱节。
// 测试里取正文 byte 长度转 u32 传给 ops（正文长度远小于 u32 上限，截断无害）。
#[allow(clippy::cast_possible_truncation)]
#[test]
fn text_modifying_commands_report_cleared_composition_consistently() {
    let (svc, _dir) = make_service();
    let session_id = svc
        .text_edit_session_create("test".to_string(), "abc".to_string(), 3, 0)
        .expect("session created");

    // 每次命令前重新 begin+update 一个活跃 composition（replace range 取当前文本末尾）。
    // 文本在命令间会变化，不能固定 offset。
    let begin_at_end = |svc: &WriterAppService, expected_revision: u64| {
        let text_len = svc.text_edit_session_get_text(session_id).len();
        let begin = svc.text_edit_session_begin_composition(
            session_id,
            text_len as u32,
            text_len as u32,
            expected_revision,
        );
        assert_eq!(begin.outcome, EditorEditOutcomeDto::Applied);
        let begin_cs = begin
            .composition_session
            .expect("begin writes composition_session");
        let upd = svc.text_edit_session_update_composition(
            session_id,
            begin_cs.session_id,
            begin_cs.generation,
            "你".to_string(),
            1,
            begin.new_revision,
        );
        assert_eq!(upd.outcome, EditorEditOutcomeDto::Applied);
        assert!(
            upd.composition.is_some(),
            "update 时 kernel 有活跃 composition，DTO 必须回填"
        );
        upd
    };
    let expect_cleared = |label: &str, dto: &crate::api::EditorEditResultDto| {
        assert!(
            dto.composition_session.is_none(),
            "{label} 清 composition 后 DTO 必须为 None（与 kernel 一致）"
        );
        assert!(
            dto.composition.is_none(),
            "{label} 清 composition 后 DTO.composition 必须为 None"
        );
    };

    // insert
    let upd = begin_at_end(&svc, 0);
    let ins = svc.text_edit_session_insert(
        session_id,
        0,
        "X".to_string(),
        crate::api::EditorTransactionCauseDto::Typing,
        upd.new_revision,
    );
    assert_eq!(ins.outcome, EditorEditOutcomeDto::Applied);
    expect_cleared("insert", &ins);
    assert!(
        svc.text_edit_session_snapshot(session_id)
            .composition
            .is_none(),
        "snapshot 与 DTO 一致：insert 后无活跃 composition"
    );

    // delete
    let upd = begin_at_end(&svc, ins.new_revision);
    let del = svc.text_edit_session_delete(
        session_id,
        1,
        2,
        crate::api::EditorTransactionCauseDto::Delete,
        upd.new_revision,
    );
    assert_eq!(del.outcome, EditorEditOutcomeDto::Applied);
    expect_cleared("delete", &del);

    // undo（undo 本身清 composition）
    let upd = begin_at_end(&svc, del.new_revision);
    let undo = svc.text_edit_session_undo(session_id, upd.new_revision);
    assert_eq!(undo.outcome, EditorEditOutcomeDto::Applied);
    expect_cleared("undo", &undo);

    // redo
    let upd = begin_at_end(&svc, undo.new_revision);
    let redo = svc.text_edit_session_redo(session_id, upd.new_revision);
    assert_eq!(redo.outcome, EditorEditOutcomeDto::Applied);
    expect_cleared("redo", &redo);

    // deleteSurrounding
    let upd = begin_at_end(&svc, redo.new_revision);
    let text_len = svc.text_edit_session_get_text(session_id).len() as u32;
    let ds = svc.text_edit_session_delete_surrounding(
        session_id,
        0,
        1,
        text_len.saturating_sub(1),
        text_len,
        crate::api::EditorTransactionCauseDto::Delete,
        upd.new_revision,
    );
    assert_eq!(ds.outcome, EditorEditOutcomeDto::Applied);
    expect_cleared("deleteSurrounding", &ds);

    // replaceAll（搜索词必须真实命中；未命中的 NoChange 不清 composition，DTO 回填）
    let upd = begin_at_end(&svc, ds.new_revision);
    let replace_all = svc.text_edit_session_replace_all(
        session_id,
        "b".to_string(),
        "z".to_string(),
        upd.new_revision,
    );
    assert_eq!(replace_all.outcome, EditorEditOutcomeDto::Applied);
    expect_cleared("replaceAll", &replace_all);

    // insertLineBreak
    let upd = begin_at_end(&svc, replace_all.new_revision);
    let ilb = svc.text_edit_session_insert_line_break(
        session_id,
        1,
        0,
        crate::api::EditorTransactionCauseDto::Typing,
        upd.new_revision,
    );
    assert_eq!(ilb.outcome, EditorEditOutcomeDto::Applied);
    expect_cleared("insertLineBreak", &ilb);

    // commitText（携带活跃 composition 会话标识）
    let upd = begin_at_end(&svc, ilb.new_revision);
    let upd_cs = upd
        .composition_session
        .expect("update writes composition_session");
    let text_len = svc.text_edit_session_get_text(session_id).len() as u32;
    let ct = svc.text_edit_session_commit_text(
        session_id,
        text_len,
        text_len,
        "AB".to_string(),
        text_len + 2,
        text_len + 2,
        upd_cs.session_id,
        upd_cs.base_revision,
        upd_cs.generation,
        crate::api::EditorTransactionCauseDto::TypingCommit,
        upd.new_revision,
    );
    assert_eq!(ct.outcome, EditorEditOutcomeDto::Applied);
    expect_cleared("commitText", &ct);
}

/// #629 评论8 第4项：cancel_composition 成功后 DTO composition 保持 None。
#[test]
fn cancel_composition_leaves_dto_composition_none() {
    let (svc, _dir) = make_service();
    let session_id = svc
        .text_edit_session_create("test".to_string(), "abc".to_string(), 3, 0)
        .expect("session created");

    let begin = svc.text_edit_session_begin_composition(session_id, 1, 2, 0);
    assert_eq!(begin.outcome, EditorEditOutcomeDto::Applied);
    let begin_cs = begin
        .composition_session
        .expect("begin writes composition_session");
    let upd = svc.text_edit_session_update_composition(
        session_id,
        begin_cs.session_id,
        begin_cs.generation,
        "你".to_string(),
        1,
        begin.new_revision,
    );
    assert_eq!(upd.outcome, EditorEditOutcomeDto::Applied);
    let upd_cs = upd
        .composition_session
        .expect("update writes composition_session");

    let cancel = svc.text_edit_session_cancel_composition(
        session_id,
        upd_cs.session_id,
        upd_cs.generation,
        upd.new_revision,
    );
    assert_eq!(cancel.outcome, EditorEditOutcomeDto::Applied);
    assert!(
        cancel.composition_session.is_none(),
        "cancel_composition clears composition_session"
    );
    assert!(
        cancel.composition.is_none(),
        "cancel_composition clears composition"
    );
    let snap = svc.text_edit_session_snapshot(session_id);
    assert!(
        snap.composition.is_none(),
        "snapshot 无 composition after cancel"
    );
}
