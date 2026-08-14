//! #624 评论8 — crop::Rope 局部编辑改造测试（TDD）。
//!
//! 覆盖：Rope UTF-8 边界访问器、光标附近 grapheme 边界迭代、
//! delta Undo/Redo、deleteSurrounding 双 delta、replace-all 多 delta、
//! composition commit delta、EditorContentDelta（CJK/emoji/空白）、
//! OffsetMap::from_single_edit/from_edits、selection-only 零 delta。

#[cfg(test)]
#[allow(clippy::module_inception)]
mod rope_tests {
    use super::super::result::EditorContentDelta;
    use super::super::result::EditorEditOutcome;
    use super::super::types::{DisplayPatch, EditorCommand, EditorOperationKind};
    use super::super::EditorKernel;
    use crate::editor::strong_types::{
        EditorRevision, EditorSessionGeneration, EditorSessionId, Utf8ByteOffset, Utf8ByteRange,
    };
    use crate::editor::transaction::{EditorTransactionCause, OffsetMap, OffsetMapKind};

    /// #624 评论10：模拟 Android 对原子 patch batch 的应用。
    ///
    /// 协议：一个 EditorEditResult 是一个原子 batch，batch 内所有 patch range 都使用
    /// base（编辑前）文档坐标；Android 按 replace_byte_range.start 降序局部应用，
    /// 右侧修改不影响左侧旧坐标。本函数复刻该应用顺序，用于断言 Core 生成的 patch
    /// 列表与最终正文（snapshot）完全一致（Core/Android mirror 一致性）。
    fn apply_patches_descending(old: &str, patches: &[DisplayPatch]) -> String {
        let mut text = old.to_string();
        let mut sorted: Vec<&DisplayPatch> = patches.iter().collect();
        sorted.sort_by_key(|p| std::cmp::Reverse(p.replace_byte_range.start().value()));
        for p in sorted {
            let start = p.replace_byte_range.start().value();
            let end = p.replace_byte_range.end().value();
            assert!(
                start <= text.len() && end <= text.len(),
                "patch 范围 [{start},{end}) 超出当前文本长度 {}",
                text.len()
            );
            text.replace_range(start..end, &p.inserted_text);
        }
        text
    }

    fn insert(kernel: &mut EditorKernel, offset: usize, text: &str) -> EditorEditOutcome {
        kernel.apply(EditorCommand::Insert {
            byte_offset: Utf8ByteOffset::unchecked(offset),
            text: text.to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: EditorRevision::new(kernel.revision()),
        })
    }

    fn delete(kernel: &mut EditorKernel, start: usize, end: usize) -> EditorEditOutcome {
        kernel.apply(EditorCommand::Delete {
            byte_range: Utf8ByteRange::from_ordered(start, end),
            deleted_text: String::new(),
            cause: EditorTransactionCause::Delete,
            expected_revision: EditorRevision::new(kernel.revision()),
        })
    }

    fn undo(kernel: &mut EditorKernel) -> EditorEditOutcome {
        kernel.apply(EditorCommand::Undo {
            expected_revision: EditorRevision::new(kernel.revision()),
        })
    }

    fn redo(kernel: &mut EditorKernel) -> EditorEditOutcome {
        kernel.apply(EditorCommand::Redo {
            expected_revision: EditorRevision::new(kernel.revision()),
        })
    }

    // ── Rope UTF-8 边界访问器 ──

    #[test]
    fn rope_accessors_report_utf8_bytes() {
        // "你好😀a\u{301}"：你=3 好=3 😀=4 a=1 combining=2 → 13 字节
        let kernel = EditorKernel::with_text("你好😀a\u{301}".to_string(), 13).unwrap();
        assert_eq!(kernel.byte_len(), 13);
        assert!(kernel.is_char_boundary(0));
        assert!(kernel.is_char_boundary(3));
        assert!(kernel.is_char_boundary(6));
        assert!(kernel.is_char_boundary(10));
        assert!(kernel.is_char_boundary(13));
        assert!(!kernel.is_char_boundary(1));
        assert!(!kernel.is_char_boundary(4));
        // combining mark（U+0301）是独立 char：11 是其起始字节，12 在它内部
        assert!(kernel.is_char_boundary(11));
        assert!(!kernel.is_char_boundary(12));
        assert_eq!(kernel.snapshot_text(), "你好😀a\u{301}");
    }

    #[test]
    fn rope_byte_slice_reads_local_range_only() {
        let kernel = EditorKernel::with_text("abcdef".to_string(), 6).unwrap();
        assert_eq!(kernel.byte_slice(1, 3).to_string(), "bc");
        assert_eq!(kernel.byte_slice(3, 6).to_string(), "def");
        // 空 slice
        assert_eq!(kernel.byte_slice(2, 2).to_string(), "");
    }

    #[test]
    fn rope_byte_slice_cjk_emoji_boundaries() {
        let kernel = EditorKernel::with_text("你😀好".to_string(), 10).unwrap();
        // 你=0..3 😀=3..7 好=7..10
        assert_eq!(kernel.byte_slice(0, 3).to_string(), "你");
        assert_eq!(kernel.byte_slice(3, 7).to_string(), "😀");
        assert_eq!(kernel.byte_slice(7, 10).to_string(), "好");
    }

    // ── grapheme 边界：从光标附近 RopeSlice 迭代，不从全文开头扫描 ──

    #[test]
    fn grapheme_boundary_crlf_is_single_cluster() {
        // "a\r\nb"：a=0, \r\n=1..3, b=3..4
        let text = "a\r\nb";
        let kernel = EditorKernel::with_text(text.to_string(), 4).unwrap();
        assert_eq!(kernel.previous_grapheme_boundary(4), 3);
        assert_eq!(kernel.previous_grapheme_boundary(3), 1);
        assert_eq!(kernel.next_grapheme_boundary(1), 3);
        assert_eq!(kernel.next_grapheme_boundary(3), 4);
    }

    #[test]
    fn grapheme_boundary_flag_emoji() {
        // "a🇨🇳b"：a=0, 🇨=1..5, 🇳=5..9, b=9..10
        let text = "a🇨🇳b";
        let kernel = EditorKernel::with_text(text.to_string(), 10).unwrap();
        assert_eq!(kernel.previous_grapheme_boundary(10), 9);
        assert_eq!(kernel.previous_grapheme_boundary(9), 1);
        assert_eq!(kernel.next_grapheme_boundary(1), 9);
        assert_eq!(kernel.next_grapheme_boundary(0), 1);
    }

    #[test]
    fn grapheme_boundary_zwj_family() {
        // "x👨\u{200D}👩\u{200D}👧y"：x=0, ZWJ family=1..19, y=19..20
        let text = "x👨\u{200D}👩\u{200D}👧y";
        let kernel = EditorKernel::with_text(text.to_string(), 20).unwrap();
        assert_eq!(kernel.previous_grapheme_boundary(20), 19);
        assert_eq!(kernel.next_grapheme_boundary(1), 19);
        // offset 在 cluster 内部（ZWJ 之后）仍回到 cluster 边界
        assert_eq!(kernel.previous_grapheme_boundary(8), 1);
        assert_eq!(kernel.next_grapheme_boundary(8), 19);
    }

    #[test]
    fn grapheme_boundary_inside_combining_cluster() {
        // "e\u{0301}x"：cluster e+combining = 0..3, x=3..4
        let text = "e\u{0301}x";
        let kernel = EditorKernel::with_text(text.to_string(), 4).unwrap();
        // offset=1（e 之后，仍在 cluster 内）
        assert_eq!(kernel.previous_grapheme_boundary(1), 0);
        assert_eq!(kernel.next_grapheme_boundary(1), 3);
        assert_eq!(kernel.next_grapheme_boundary(3), 4);
    }

    // ── EditorContentDelta：insert/delete/replace/空白/emoji ──

    #[test]
    fn content_delta_insert_cjk_and_whitespace() {
        let mut kernel = EditorKernel::new();
        let result = insert(&mut kernel, 0, "你好 ").into_result();
        // 你、好、空格 = 3 个 char；非空白 = 2
        assert_eq!(
            result.content_delta,
            EditorContentDelta::from_parts(3, 0, 2, 0)
        );
    }

    #[test]
    fn content_delta_insert_emoji_is_single_char() {
        let mut kernel = EditorKernel::new();
        let result = insert(&mut kernel, 0, "😀").into_result();
        // 😀 是 1 个 Unicode scalar（4 UTF-8 bytes），不是 2 个
        assert_eq!(result.content_delta.inserted_chars, 1);
        assert_eq!(result.content_delta.inserted_non_whitespace_chars, 1);
    }

    #[test]
    fn content_delta_delete_newline_is_whitespace() {
        let mut kernel = EditorKernel::with_text("你好\n世界".to_string(), 10).unwrap();
        let result = delete(&mut kernel, 6, 7).into_result();
        assert_eq!(result.content_delta.inserted_chars, 0);
        assert_eq!(result.content_delta.deleted_chars, 1);
        assert_eq!(result.content_delta.deleted_non_whitespace_chars, 0);
    }

    #[test]
    fn content_delta_replace_counts_both_sides() {
        let mut kernel = EditorKernel::with_text("abc".to_string(), 3).unwrap();
        let result = kernel
            .apply(EditorCommand::Replace {
                byte_range: Utf8ByteRange::from_ordered(1, 2),
                replacement_text: "XY".to_string(),
                original_text: "b".to_string(),
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(kernel.revision()),
            })
            .into_result();
        assert_eq!(
            result.content_delta,
            EditorContentDelta::from_parts(2, 1, 2, 1)
        );
    }

    #[test]
    fn content_delta_selection_only_is_zero() {
        let mut kernel = EditorKernel::with_text("hello".to_string(), 5).unwrap();
        let result = kernel
            .apply(EditorCommand::SetSelection {
                anchor: Utf8ByteOffset::unchecked(0),
                head: Utf8ByteOffset::unchecked(3),
                expected_revision: EditorRevision::new(kernel.revision()),
            })
            .into_result();
        assert_eq!(result.content_delta, EditorContentDelta::default());
        assert!(result.display_patches.is_empty());
    }

    #[test]
    fn content_delta_composition_update_is_zero() {
        let mut kernel = EditorKernel::with_text("hello".to_string(), 5).unwrap();
        let begin = kernel
            .apply(EditorCommand::BeginComposition {
                replace_range: Utf8ByteRange::from_ordered(5, 5),
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        let (session_id, _, generation) = kernel.composition_session_info().unwrap();
        let update = kernel
            .apply(EditorCommand::UpdateComposition {
                composition_session_id: EditorSessionId::new(session_id),
                composition_generation: EditorSessionGeneration::new(generation),
                new_preedit_text: "世界".to_string(),
                new_preedit_cursor_offset: Utf8ByteOffset::unchecked(4),
                expected_revision: begin.new_revision,
            })
            .into_result();
        // 预输入更新不写正文 → delta 必须为 0
        assert_eq!(update.content_delta, EditorContentDelta::default());
        assert_eq!(kernel.snapshot_text(), "hello");
    }

    #[test]
    fn content_delta_composition_commit_counts_committed_text() {
        let mut kernel = EditorKernel::with_text("你好".to_string(), 6).unwrap();
        let begin = kernel
            .apply(EditorCommand::BeginComposition {
                replace_range: Utf8ByteRange::from_ordered(6, 6),
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        let (session_id, _, generation) = kernel.composition_session_info().unwrap();
        let _ = kernel
            .apply(EditorCommand::UpdateComposition {
                composition_session_id: EditorSessionId::new(session_id),
                composition_generation: EditorSessionGeneration::new(generation),
                new_preedit_text: "世界".to_string(),
                new_preedit_cursor_offset: Utf8ByteOffset::unchecked(4),
                expected_revision: begin.new_revision,
            })
            .into_result();
        let (_, _, gen2) = kernel.composition_session_info().unwrap();
        let finish = kernel
            .apply(EditorCommand::FinishComposition {
                composition_session_id: EditorSessionId::new(session_id),
                composition_generation: EditorSessionGeneration::new(gen2),
                expected_revision: begin.new_revision,
            })
            .into_result();
        assert_eq!(kernel.snapshot_text(), "你好世界");
        assert_eq!(finish.content_delta.inserted_chars, 2);
        assert_eq!(finish.content_delta.inserted_non_whitespace_chars, 2);
        assert_eq!(finish.content_delta.deleted_chars, 0);
    }

    // ── delta Undo/Redo ──

    #[test]
    fn undo_redo_single_insert_delta() {
        let mut kernel = EditorKernel::with_text("ab".to_string(), 2).unwrap();
        let _r1 = insert(&mut kernel, 2, "c").into_result();
        assert_eq!(kernel.snapshot_text(), "abc");

        let undid = undo(&mut kernel).into_result();
        assert_eq!(kernel.snapshot_text(), "ab");
        assert_eq!(kernel.cursor(), 2);
        // undo 实际删除 1 个字符
        assert_eq!(undid.content_delta.deleted_chars, 1);
        assert_eq!(undid.content_delta.inserted_chars, 0);
        assert_eq!(undid.visual_intent.cause, EditorTransactionCause::Undo);

        let redid = redo(&mut kernel).into_result();
        assert_eq!(kernel.snapshot_text(), "abc");
        assert_eq!(kernel.cursor(), 3);
        assert_eq!(redid.content_delta.inserted_chars, 1);
        assert_eq!(redid.content_delta.deleted_chars, 0);
    }

    #[test]
    fn undo_redo_delete_delta() {
        let mut kernel = EditorKernel::with_text("你好世界".to_string(), 12).unwrap();
        let _r1 = delete(&mut kernel, 6, 12).into_result();
        assert_eq!(kernel.snapshot_text(), "你好");
        // 删除的"世界"是可 undo 的：undo 插入 2 chars
        let undid = undo(&mut kernel).into_result();
        assert_eq!(kernel.snapshot_text(), "你好世界");
        assert_eq!(undid.content_delta.inserted_chars, 2);
        let redid = redo(&mut kernel).into_result();
        assert_eq!(kernel.snapshot_text(), "你好");
        assert_eq!(redid.content_delta.deleted_chars, 2);
    }

    #[test]
    fn undo_restores_cursor_to_old_selection() {
        let mut kernel = EditorKernel::with_text("abc".to_string(), 3).unwrap();
        let _ = kernel
            .apply(EditorCommand::SetSelection {
                anchor: Utf8ByteOffset::unchecked(0),
                head: Utf8ByteOffset::unchecked(2),
                expected_revision: EditorRevision::new(kernel.revision()),
            })
            .into_result();
        let _ = kernel
            .apply(EditorCommand::Replace {
                byte_range: Utf8ByteRange::from_ordered(0, 2),
                replacement_text: "X".to_string(),
                original_text: "ab".to_string(),
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(kernel.revision()),
            })
            .into_result();
        assert_eq!(kernel.snapshot_text(), "Xc");
        let undid = undo(&mut kernel).into_result();
        assert_eq!(kernel.snapshot_text(), "abc");
        // undo 恢复编辑前选区（anchor=0, head=2）
        assert_eq!(kernel.selection(), (0, 2));
        assert_eq!(undid.visual_intent.cause, EditorTransactionCause::Undo);
    }

    // ── deleteSurrounding：两个 delta ──

    #[test]
    fn delete_surrounding_two_deltas_undo_redo() {
        // "abXYcd"：光标 3..3（Y 后）。before=[2,3)="X"，after=[3,4)="c"。
        // 删除后 "abYd"。
        let mut kernel = EditorKernel::with_text("abXYcd".to_string(), 3).unwrap();
        let r1 = kernel
            .apply(EditorCommand::DeleteSurrounding {
                before_byte_range: Utf8ByteRange::from_ordered(2, 3),
                after_byte_range: Utf8ByteRange::from_ordered(4, 5),
                cause: EditorTransactionCause::Delete,
                expected_revision: EditorRevision::new(kernel.revision()),
            })
            .into_result();
        assert_eq!(kernel.snapshot_text(), "abYd");
        assert_eq!(kernel.cursor(), 2);
        // 两个 delta 各删除 1 个字符
        assert_eq!(r1.content_delta.deleted_chars, 2);
        assert_eq!(r1.content_delta.inserted_chars, 0);
        // #624 评论10：原子 patch batch — 每条 delta 一条局部 DisplayPatch
        // （base 文档坐标，删除的 inserted_text 为空）。batch 内顺序不构成协议
        // （Android 按 start 降序应用），这里按 start 排序后断言两条局部 patch。
        let mut ranges: Vec<Utf8ByteRange> = r1
            .display_patches
            .iter()
            .map(|p| p.replace_byte_range)
            .collect();
        ranges.sort_by_key(|r| r.start().value());
        assert_eq!(r1.display_patches.len(), 2);
        assert_eq!(ranges[0], Utf8ByteRange::from_ordered(2, 3));
        assert_eq!(ranges[1], Utf8ByteRange::from_ordered(4, 5));
        assert!(r1
            .display_patches
            .iter()
            .all(|p| p.inserted_text.is_empty()));
        assert_eq!(
            r1.display_patches[0].base_revision,
            r1.display_patches[1].base_revision
        );
        assert_eq!(
            r1.display_patches[0].new_revision,
            r1.display_patches[1].new_revision
        );

        // undo：两个 delta 逆序恢复 → "abXYcd"
        let undid = undo(&mut kernel).into_result();
        assert_eq!(kernel.snapshot_text(), "abXYcd");
        assert_eq!(kernel.cursor(), 3);
        assert_eq!(undid.content_delta.inserted_chars, 2);

        // redo：再次删除 → "abYd"
        let redid = redo(&mut kernel).into_result();
        assert_eq!(kernel.snapshot_text(), "abYd");
        assert_eq!(redid.content_delta.deleted_chars, 2);
    }

    #[test]
    fn delete_surrounding_before_only_single_delta() {
        let mut kernel = EditorKernel::with_text("ab cd".to_string(), 3).unwrap();
        let r1 = kernel
            .apply(EditorCommand::DeleteSurrounding {
                before_byte_range: Utf8ByteRange::from_ordered(2, 3),
                after_byte_range: Utf8ByteRange::zero(),
                cause: EditorTransactionCause::Delete,
                expected_revision: EditorRevision::new(kernel.revision()),
            })
            .into_result();
        assert_eq!(kernel.snapshot_text(), "abcd");
        assert_eq!(r1.content_delta.deleted_chars, 1);
        let undid = undo(&mut kernel).into_result();
        assert_eq!(kernel.snapshot_text(), "ab cd");
        assert_eq!(undid.content_delta.inserted_chars, 1);
    }

    #[test]
    fn delete_surrounding_patches_apply_to_base_coords() {
        // "abXYcd"：before=[2,3)="X"，after=[4,5)="c" → 两条 base 坐标局部 patch。
        // Android 按 start 降序应用后必须与 snapshot "abYd" 一致（mirror 一致性）。
        let mut kernel = EditorKernel::with_text("abXYcd".to_string(), 3).unwrap();
        let r1 = kernel
            .apply(EditorCommand::DeleteSurrounding {
                before_byte_range: Utf8ByteRange::from_ordered(2, 3),
                after_byte_range: Utf8ByteRange::from_ordered(4, 5),
                cause: EditorTransactionCause::Delete,
                expected_revision: EditorRevision::new(kernel.revision()),
            })
            .into_result();
        assert_eq!(kernel.snapshot_text(), "abYd");
        assert_eq!(r1.display_patches.len(), 2);
        assert_eq!(
            apply_patches_descending("abXYcd", &r1.display_patches),
            kernel.snapshot_text()
        );
    }

    /// #624 评论10 第4项补漏：deleteSurrounding 双 delta 的 **undo** patch 必须用
    /// 最终文本坐标。旧实现 after delta 的 new_range=point(as_) 是在「仅删除 after、
    /// before 尚未删除」的时刻计算的；before 删除后该点在最终文本中左移
    /// before_deleted_len。顺序应用 undo delta（先恢复 before）恰好补偿正确，但 undo
    /// DisplayPatch 被 Android 按 base 坐标降序独立应用时 after patch 会插到错误位置，
    /// Core/Android mirror 分裂（"abXYcd" → undo 后 Android 得到 "abXYdc"）。
    #[test]
    fn delete_surrounding_undo_patches_apply_to_final_coords() {
        // "abXYcd"：光标 3。before=[2,3)="X"，after=[4,5)="c" → 删除后 "abYd"。
        let mut kernel = EditorKernel::with_text("abXYcd".to_string(), 3).unwrap();
        let r1 = kernel
            .apply(EditorCommand::DeleteSurrounding {
                before_byte_range: Utf8ByteRange::from_ordered(2, 3),
                after_byte_range: Utf8ByteRange::from_ordered(4, 5),
                cause: EditorTransactionCause::Delete,
                expected_revision: EditorRevision::new(kernel.revision()),
            })
            .into_result();
        assert_eq!(kernel.snapshot_text(), "abYd");
        // 前向 patch 也是 base 坐标 batch：降序应用与 snapshot 一致。
        assert_eq!(r1.display_patches.len(), 2);
        assert_eq!(
            apply_patches_descending("abXYcd", &r1.display_patches),
            kernel.snapshot_text()
        );
        // after delta 的 new_range 必须是最终文本坐标：after 点(4) 左移 Lb=1 → 3。
        let undid = undo(&mut kernel).into_result();
        assert_eq!(kernel.snapshot_text(), "abXYcd");
        // undo patch 按 base（undo 前 = 最终文本 "abYd"）坐标降序应用必须与 snapshot 一致。
        assert_eq!(
            apply_patches_descending("abYd", &undid.display_patches),
            kernel.snapshot_text()
        );
    }

    /// #624 评论10 第4项补漏：before/after 紧邻（as_ == be）时两个 undo patch 在
    /// 同一位置（均退化为 point(bs)），Android 稳定降序按列表顺序应用 — after（右侧）
    /// 必须排在 before 前面，否则 "b"、"c" 插入顺序颠倒成 "cb"。
    #[test]
    fn delete_surrounding_adjacent_undo_patch_order() {
        // "abcd"：光标 2。before=[1,2)="b"，after=[2,3)="c"（紧邻）→ "ad"。
        let mut kernel = EditorKernel::with_text("abcd".to_string(), 2).unwrap();
        let r1 = kernel
            .apply(EditorCommand::DeleteSurrounding {
                before_byte_range: Utf8ByteRange::from_ordered(1, 2),
                after_byte_range: Utf8ByteRange::from_ordered(2, 3),
                cause: EditorTransactionCause::Delete,
                expected_revision: EditorRevision::new(kernel.revision()),
            })
            .into_result();
        assert_eq!(kernel.snapshot_text(), "ad");
        assert_eq!(r1.display_patches.len(), 2);
        assert_eq!(
            apply_patches_descending("abcd", &r1.display_patches),
            kernel.snapshot_text()
        );

        let undid = undo(&mut kernel).into_result();
        assert_eq!(kernel.snapshot_text(), "abcd");
        // 两个 undo patch 都退化为 point(1)；按列表顺序（after 在前）应用才得 "abcd"。
        assert_eq!(
            apply_patches_descending("ad", &undid.display_patches),
            kernel.snapshot_text()
        );

        let redid = redo(&mut kernel).into_result();
        assert_eq!(kernel.snapshot_text(), "ad");
        assert_eq!(
            apply_patches_descending("abcd", &redid.display_patches),
            kernel.snapshot_text()
        );
    }

    /// #624 评论10 第4项复审补漏：相邻 deleteSurrounding undo 的 OffsetMap 尾段映射。
    ///
    /// "abcd" 光标 2，before=[1,2)="b"、after=[2,3)="c"（紧邻）→ "ad"。undo 时两条
    /// inverse delta 的 new_range 都退化为 point(1)（同点零长编辑），`from_edits` 必须
    /// 取两条端点中的**最大值**推进 new_pos：尾段保留字符 'd'（old 1）应映射到 undo 后
    /// 文本 "abcd" 的 new offset 3（"b"+"c" 都插入到 point 1）。旧实现顺序赋值
    /// `new_pos = new_end`，后处理的 before 端点 2 覆盖 after 的 3，尾段映射偏移 1 字节
    /// （动画 OffsetMap 坐标错误，不影响正文/mirror）。
    #[test]
    fn delete_surrounding_adjacent_undo_offset_map_tail_maps_to_final() {
        let mut kernel = EditorKernel::with_text("abcd".to_string(), 2).unwrap();
        kernel
            .apply(EditorCommand::DeleteSurrounding {
                before_byte_range: Utf8ByteRange::from_ordered(1, 2),
                after_byte_range: Utf8ByteRange::from_ordered(2, 3),
                cause: EditorTransactionCause::Delete,
                expected_revision: EditorRevision::new(kernel.revision()),
            })
            .into_result();
        assert_eq!(kernel.snapshot_text(), "ad");

        let undid = undo(&mut kernel).into_result();
        assert_eq!(kernel.snapshot_text(), "abcd");
        let map = undid
            .visual_intent
            .offset_map
            .expect("undo 必须携带 OffsetMap");
        // old = undo 前文本 "ad"：头部 [0,1) 恒等；'d' 在 old 1，undo 后 "abcd" 中在 new 3。
        assert_eq!(map.map_old_to_new(0), Some(0));
        assert_eq!(map.map_old_to_new(1), Some(3));
    }

    // ── replace-all：多 delta ──

    #[test]
    fn replace_all_multi_delta_undo_redo() {
        let mut kernel = EditorKernel::with_text("aXbXc".to_string(), 5).unwrap();
        let r1 = kernel
            .apply(EditorCommand::ReplaceAll {
                search: "X".to_string(),
                replacement: "YY".to_string(),
                expected_revision: EditorRevision::new(kernel.revision()),
            })
            .into_result();
        assert_eq!(kernel.snapshot_text(), "aYYbYYc");
        // 替换 2 处：删除 2 chars、插入 4 chars
        assert_eq!(r1.content_delta.deleted_chars, 2);
        assert_eq!(r1.content_delta.inserted_chars, 4);
        assert_eq!(r1.visual_intent.operation_kind, EditorOperationKind::Format);

        let undid = undo(&mut kernel).into_result();
        assert_eq!(kernel.snapshot_text(), "aXbXc");
        // undo：插入原被替换的 "X"×2，删除原插入的 "YY"×2
        assert_eq!(undid.content_delta.inserted_chars, 2);
        assert_eq!(undid.content_delta.deleted_chars, 4);

        let redid = redo(&mut kernel).into_result();
        assert_eq!(kernel.snapshot_text(), "aYYbYYc");
        assert_eq!(redid.content_delta.inserted_chars, 4);
        assert_eq!(redid.content_delta.deleted_chars, 2);
    }

    #[test]
    fn replace_all_shorter_replacement_undo_redo() {
        // 替换变短（"XX"→"x"）：new_range 必须正确收缩，不能 usize 溢出。
        let mut kernel = EditorKernel::with_text("aXXbXXc".to_string(), 7).unwrap();
        let r1 = kernel
            .apply(EditorCommand::ReplaceAll {
                search: "XX".to_string(),
                replacement: "x".to_string(),
                expected_revision: EditorRevision::new(kernel.revision()),
            })
            .into_result();
        assert_eq!(kernel.snapshot_text(), "axbxc");
        assert_eq!(r1.content_delta.deleted_chars, 4);
        assert_eq!(r1.content_delta.inserted_chars, 2);
        // undo 恢复原文
        let undid = undo(&mut kernel).into_result();
        assert_eq!(kernel.snapshot_text(), "aXXbXXc");
        assert_eq!(undid.content_delta.inserted_chars, 4);
        assert_eq!(undid.content_delta.deleted_chars, 2);
        // redo 再次收缩
        let redid = redo(&mut kernel).into_result();
        assert_eq!(kernel.snapshot_text(), "axbxc");
        assert_eq!(redid.content_delta.inserted_chars, 2);
        assert_eq!(redid.content_delta.deleted_chars, 4);
        // undo 补丁范围：undo 前文本中 [1,2) 与 [4,5) 被替换回 "XX"
        assert_eq!(kernel.snapshot_text(), "axbxc");
        let undid2 = undo(&mut kernel).into_result();
        assert_eq!(kernel.snapshot_text(), "aXXbXXc");
        assert_eq!(undid2.display_patches.len(), 2);
    }

    #[test]
    fn replace_all_patches_are_local_per_match() {
        // aXbXc + X→YY：Core 正文正确是 aYYbYYc。patch 必须是每条匹配一条局部
        // patch（base 坐标 [1,2) 与 [3,4)），不再合成覆盖 [1,4) 的外层 patch
        // （旧实现的 saturating_sub 长度差在替换变长时得到错误 retained）。
        let mut kernel = EditorKernel::with_text("aXbXc".to_string(), 5).unwrap();
        let r1 = kernel
            .apply(EditorCommand::ReplaceAll {
                search: "X".to_string(),
                replacement: "YY".to_string(),
                expected_revision: EditorRevision::new(kernel.revision()),
            })
            .into_result();
        assert_eq!(kernel.snapshot_text(), "aYYbYYc");
        assert_eq!(r1.display_patches.len(), 2);
        assert_eq!(
            r1.display_patches[0].replace_byte_range,
            Utf8ByteRange::from_ordered(1, 2)
        );
        assert_eq!(r1.display_patches[0].inserted_text, "YY");
        assert_eq!(
            r1.display_patches[1].replace_byte_range,
            Utf8ByteRange::from_ordered(3, 4)
        );
        assert_eq!(r1.display_patches[1].inserted_text, "YY");
        // batch 内所有 patch 携带同一组 base/new revision（原子 batch 边界）。
        assert_eq!(
            r1.display_patches[0].base_revision,
            r1.display_patches[1].base_revision
        );
        assert_eq!(
            r1.display_patches[0].new_revision,
            r1.display_patches[1].new_revision
        );
        // Core/Android mirror 一致性：降序局部应用 == snapshot。
        assert_eq!(
            apply_patches_descending("aXbXc", &r1.display_patches),
            kernel.snapshot_text()
        );
    }

    #[test]
    fn replace_all_variable_length_and_far_apart() {
        // 缩短："aXXbXXc" + XX→x → 两条局部 patch（[1,3)→"x"、[4,6)→"x"）。
        let mut kernel = EditorKernel::with_text("aXXbXXc".to_string(), 7).unwrap();
        let r1 = kernel
            .apply(EditorCommand::ReplaceAll {
                search: "XX".to_string(),
                replacement: "x".to_string(),
                expected_revision: EditorRevision::new(kernel.revision()),
            })
            .into_result();
        assert_eq!(kernel.snapshot_text(), "axbxc");
        assert_eq!(r1.display_patches.len(), 2);
        assert_eq!(
            r1.display_patches[0].replace_byte_range,
            Utf8ByteRange::from_ordered(1, 3)
        );
        assert_eq!(r1.display_patches[0].inserted_text, "x");
        assert_eq!(
            r1.display_patches[1].replace_byte_range,
            Utf8ByteRange::from_ordered(4, 6)
        );
        assert_eq!(r1.display_patches[1].inserted_text, "x");
        assert_eq!(
            apply_patches_descending("aXXbXXc", &r1.display_patches),
            kernel.snapshot_text()
        );

        // 变长且两处相距远："aXbcXdef" + X→YYY → patch [1,2) 与 [4,5)，
        // 中间保留正文不得被复制进任何一条 patch。
        let mut kernel2 = EditorKernel::with_text("aXbcXdef".to_string(), 8).unwrap();
        let r2 = kernel2
            .apply(EditorCommand::ReplaceAll {
                search: "X".to_string(),
                replacement: "YYY".to_string(),
                expected_revision: EditorRevision::new(kernel2.revision()),
            })
            .into_result();
        assert_eq!(kernel2.snapshot_text(), "aYYYbcYYYdef");
        assert_eq!(r2.display_patches.len(), 2);
        assert_eq!(
            r2.display_patches[0].replace_byte_range,
            Utf8ByteRange::from_ordered(1, 2)
        );
        assert_eq!(r2.display_patches[0].inserted_text, "YYY");
        assert_eq!(
            r2.display_patches[1].replace_byte_range,
            Utf8ByteRange::from_ordered(4, 5)
        );
        assert_eq!(r2.display_patches[1].inserted_text, "YYY");
        assert_eq!(
            apply_patches_descending("aXbcXdef", &r2.display_patches),
            kernel2.snapshot_text()
        );
    }

    /// #624 评论10 第4项：replace-all 的 **undo** patch 使用 undo 前文本（= 替换后
    /// 最终文本）坐标。变长替换时 new_range 由累计长度差决定（X→YY 时第二处
    /// 从 [3,4) 变成 [4,6)），undo batch 按 start 降序应用必须与 Core snapshot 一致
    /// （mirror 一致性）；旧实现若把 new_range 误写成 base 坐标，降序应用会得到
    /// "aXbYXc" 之类的分裂正文。
    #[test]
    fn replace_all_undo_patches_apply_to_final_coords() {
        // "aXbXc" + X→YY → "aYYbYYc"（undo 前文本，patch base 坐标）。
        let mut kernel = EditorKernel::with_text("aXbXc".to_string(), 5).unwrap();
        let r1 = kernel
            .apply(EditorCommand::ReplaceAll {
                search: "X".to_string(),
                replacement: "YY".to_string(),
                expected_revision: EditorRevision::new(kernel.revision()),
            })
            .into_result();
        assert_eq!(kernel.snapshot_text(), "aYYbYYc");
        // 前向 patch 是 base（编辑前 "aXbXc"）坐标局部列表：降序应用与 snapshot 一致。
        assert_eq!(
            apply_patches_descending("aXbXc", &r1.display_patches),
            kernel.snapshot_text()
        );

        let undid = undo(&mut kernel).into_result();
        assert_eq!(kernel.snapshot_text(), "aXbXc");
        // undo patch 是 undo 前文本（"aYYbYYc"）坐标：变长替换后第二处起点右移 1
        // （[4,6)），第一处 [1,3)。列表按 start 降序（与 undo_order 一致）。
        assert_eq!(undid.display_patches.len(), 2);
        assert_eq!(
            undid.display_patches[0].replace_byte_range,
            Utf8ByteRange::from_ordered(4, 6)
        );
        assert_eq!(undid.display_patches[0].inserted_text, "X");
        assert_eq!(
            undid.display_patches[1].replace_byte_range,
            Utf8ByteRange::from_ordered(1, 3)
        );
        assert_eq!(undid.display_patches[1].inserted_text, "X");
        // 降序应用 undo batch 必须还原原文（mirror 一致性）。
        assert_eq!(
            apply_patches_descending("aYYbYYc", &undid.display_patches),
            kernel.snapshot_text()
        );
    }

    #[test]
    fn replace_all_no_match_is_no_change() {
        let mut kernel = EditorKernel::with_text("abc".to_string(), 3).unwrap();
        let result = kernel
            .apply(EditorCommand::ReplaceAll {
                search: "Z".to_string(),
                replacement: "Y".to_string(),
                expected_revision: EditorRevision::new(kernel.revision()),
            })
            .into_result();
        assert_eq!(result.content_delta, EditorContentDelta::default());
        assert!(result.display_patches.is_empty());
        assert_eq!(kernel.snapshot_text(), "abc");
    }

    // ── OffsetMap::from_single_edit / from_edits ──

    #[test]
    fn offset_map_from_single_edit_insert() {
        // old "ab" → offset 1 插入 "X" → new "aXb"
        let map = OffsetMap::from_single_edit(2, (1, 1), 1);
        assert_eq!(map.entries.len(), 2);
        assert_eq!(map.entries[0].old_byte_offset.value(), 0);
        assert_eq!(map.entries[0].new_byte_offset.value(), 0);
        assert_eq!(map.entries[0].length, 1);
        assert_eq!(map.entries[0].kind, OffsetMapKind::Identity);
        assert_eq!(map.entries[1].old_byte_offset.value(), 1);
        assert_eq!(map.entries[1].new_byte_offset.value(), 2);
        assert_eq!(map.entries[1].length, 1);
        assert_eq!(map.entries[1].kind, OffsetMapKind::Shifted);
        assert_eq!(map.map_old_to_new(0), Some(0));
        assert_eq!(map.map_old_to_new(1), Some(2));
    }

    #[test]
    fn offset_map_from_single_edit_delete() {
        // old "aXb" → 删除 [1,2) → new "ab"
        let map = OffsetMap::from_single_edit(3, (1, 2), 0);
        assert_eq!(map.entries.len(), 2);
        assert_eq!(map.entries[0].kind, OffsetMapKind::Identity);
        assert_eq!(map.entries[0].length, 1);
        assert_eq!(map.entries[1].old_byte_offset.value(), 2);
        assert_eq!(map.entries[1].new_byte_offset.value(), 1);
        assert_eq!(map.entries[1].length, 1);
        assert_eq!(map.entries[1].kind, OffsetMapKind::Shifted);
        assert_eq!(map.map_old_to_new(2), Some(1));
    }

    #[test]
    fn offset_map_from_single_edit_replace() {
        // old "abc" → 替换 [1,2) 为 "XY" → new "aXYc"
        let map = OffsetMap::from_single_edit(3, (1, 2), 2);
        assert_eq!(map.entries.len(), 2);
        assert_eq!(map.entries[0].length, 1);
        assert_eq!(map.entries[1].old_byte_offset.value(), 2);
        assert_eq!(map.entries[1].new_byte_offset.value(), 3);
        assert_eq!(map.entries[1].length, 1);
    }

    #[test]
    fn offset_map_from_single_edit_no_identity_prefix() {
        // old "abc" → 替换 [0,3) 为 "x" → 无静态区域
        let map = OffsetMap::from_single_edit(3, (0, 3), 1);
        assert!(map.entries.is_empty());
    }

    #[test]
    fn offset_map_from_edits_two_deletes() {
        // old "abXYcd" → 删除 [2,3)="X" 和 [4,5)="c"，保留 "Y" → new "abYd"
        let map = OffsetMap::from_edits(6, &[(2, 3, 2, 2), (4, 5, 3, 3)]);
        // 区域：[0,2) Identity；[3,4)（Y）→new [2,3) Shifted；[5,6)（d）→new [3,4) Shifted
        assert_eq!(map.entries.len(), 3);
        assert_eq!(map.entries[0].old_byte_offset.value(), 0);
        assert_eq!(map.entries[0].new_byte_offset.value(), 0);
        assert_eq!(map.entries[0].length, 2);
        assert_eq!(map.entries[0].kind, OffsetMapKind::Identity);
        assert_eq!(map.entries[1].old_byte_offset.value(), 3);
        assert_eq!(map.entries[1].new_byte_offset.value(), 2);
        assert_eq!(map.entries[1].length, 1);
        assert_eq!(map.entries[1].kind, OffsetMapKind::Shifted);
        assert_eq!(map.entries[2].old_byte_offset.value(), 5);
        assert_eq!(map.entries[2].new_byte_offset.value(), 3);
        assert_eq!(map.entries[2].length, 1);
        assert_eq!(map.entries[2].kind, OffsetMapKind::Shifted);
        assert_eq!(map.map_old_to_new(3), Some(2));
        assert_eq!(map.map_old_to_new(5), Some(3));
    }

    // ── 热路径不得全文 clone 的守卫测试 ──

    #[test]
    fn insert_keeps_byte_len_local_and_content_delta_exact() {
        // 长文本中部插入：byte_len 精确、delta 精确，不依赖全文一致性扫描
        let mut kernel =
            EditorKernel::with_text("a".repeat(1000).as_str().to_string(), 1000).unwrap();
        let before = kernel.byte_len();
        let r1 = insert(&mut kernel, 500, "中文").into_result();
        assert_eq!(kernel.byte_len(), before + "中文".len());
        assert_eq!(r1.content_delta.inserted_chars, 2);
        assert_eq!(r1.content_delta.inserted_non_whitespace_chars, 2);
        // 局部 patch 只覆盖插入点
        assert_eq!(r1.display_patches.len(), 1);
        assert_eq!(
            r1.display_patches[0].replace_byte_range.start().value(),
            500
        );
        assert_eq!(r1.display_patches[0].inserted_text, "中文");
        // undo 后长度恢复
        let undid = undo(&mut kernel).into_result();
        assert_eq!(kernel.byte_len(), before);
        assert_eq!(undid.content_delta.deleted_chars, 2);
    }

    #[test]
    fn replace_inside_long_text_produces_local_patch() {
        let mut kernel = EditorKernel::with_text(
            "a".repeat(500).as_str().to_string() + "XYZ" + &"b".repeat(500),
            1000,
        )
        .unwrap();
        let r1 = kernel
            .apply(EditorCommand::Replace {
                byte_range: Utf8ByteRange::from_ordered(500, 503),
                replacement_text: "好".to_string(),
                original_text: "XYZ".to_string(),
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(kernel.revision()),
            })
            .into_result();
        assert_eq!(kernel.byte_len(), 1003);
        assert_eq!(r1.display_patches.len(), 1);
        assert_eq!(
            r1.display_patches[0].replace_byte_range.start().value(),
            500
        );
        assert_eq!(r1.display_patches[0].replace_byte_range.end().value(), 503);
        assert_eq!(r1.display_patches[0].inserted_text, "好");
        assert_eq!(r1.content_delta.deleted_chars, 3);
        assert_eq!(r1.content_delta.inserted_chars, 1);
    }
}
