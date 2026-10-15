// ── Qt 文本布局模块：affected paragraph ranges ──

/// Issue #710 评论 5731145076 症状四/五: 根据 old/new text 和改动位置计算
/// affected paragraph ranges。
///
/// 之前 pipeline.rs 直接用 `vt.inserted_range.or(vt.deleted_range)` 取
/// affected_byte_start/end，对于 "\n" 插入/删除，inserted_range/deleted_range
/// 只是 "\n" 的 1 byte range，不覆盖换行后所有受重排影响的段落。导致
/// `prepare_affected_paragraphs_visual_snapshot` 只排版 "\n" 所在段落，
/// 换行后的行重排依赖 reflow 但 reflow 只处理 unchanged material，
/// 文字闪烁/光标乱闪。
///
/// 本函数根据 old/new text 的段落边界扩展 affected range：
/// - 找到 edit_start 所在段落的起始 byte
/// - 找到 edit_end 所在段落的结束 byte（含 '\n'）
/// - old 侧和 new 侧分别按各自段落边界计算
///
/// 返回 (old_start, old_end, new_start, new_end)。
/// old 侧用 old_text 段落边界，new 侧用 new_text 段落边界，
/// 不拿同一组 byte start/end 同时套 old/new 两份正文。
///
/// Issue #710 评论 5732160521 问题 1: 接口改成显式区分 old/new 坐标系。
/// 之前 `(old_text, new_text, edit_start, edit_end)` 把同一组 byte 坐标
/// 同时套给 old/new text，但 `inserted_range` 是新文本坐标、`deleted_range`
/// 是旧文本坐标，不能互换。现在 `old_edit_range` 用于 old_text 段落扩展，
/// `new_edit_range` 用于 new_text 段落扩展，调用方按事务类型分别传正确坐标系。
pub fn compute_affected_paragraph_ranges(
    old_text: &str,
    new_text: &str,
    old_edit_range: (usize, usize),
    new_edit_range: (usize, usize),
) -> (usize, usize, usize, usize) {
    // old 侧：用 old_edit_range 在 old_text 坐标系扩展段落边界
    let (old_start, old_end) =
        expand_to_paragraph_boundaries(old_text, old_edit_range.0, old_edit_range.1);
    // new 侧：用 new_edit_range 在 new_text 坐标系扩展段落边界
    let (new_start, new_end) =
        expand_to_paragraph_boundaries(new_text, new_edit_range.0, new_edit_range.1);
    (old_start, old_end, new_start, new_end)
}

/// 将 byte range 扩展到包含它的完整段落边界。
///
/// 段落以 '\n' 分隔。返回的 range 包含从 edit_start 所在段落的起始
/// 到 edit_end 所在段落的结束（含 '\n'）。
///
/// Issue #710 评论 5732160521 问题 1: 切片前必须保证 byte offset 是该字符串
/// 自己的 UTF-8 char boundary。之前直接 `text[e..]` 在 byte offset 落在多字节
/// UTF-8 字符内部时 panic（如 "甲"[1..]）。现在用 `floor_char_boundary` 把
/// offset 调整到最近的 char boundary，再切片。`str::floor_char_boundary`
/// 自 Rust 1.73 起稳定。
fn expand_to_paragraph_boundaries(text: &str, start: usize, end: usize) -> (usize, usize) {
    // 先把 start/end 钳到 [0, text.len()]，再调整到 char boundary。
    // floor_char_boundary(0) == 0，floor_char_boundary(len) == len，边界安全。
    let s = text.floor_char_boundary(start.min(text.len()));
    let e = text.floor_char_boundary(end.min(text.len()));
    if s > e {
        return (e, s);
    }
    // 找 start 所在段落的起始：往前找第一个 '\n' 的下一个位置
    let para_start = if s == 0 {
        0
    } else {
        // 在 text[..s] 中找最后一个 '\n'，段落起始是它后面
        text[..s].rfind('\n').map(|p| p + 1).unwrap_or(0)
    };
    // 找 end 所在段落的结束：往后找第一个 '\n'（含）
    let para_end = if e >= text.len() {
        text.len()
    } else {
        // 在 text[e..] 中找第一个 '\n'，段落结束是它后面（含 '\n'）
        text[e..]
            .find('\n')
            .map(|p| e + p + 1)
            .unwrap_or(text.len())
    };
    (para_start, para_end)
}

// ── Issue #710 评论 5732160521 回归测试 ──
//
// 问题 1: compute_affected_paragraph_ranges 坐标系混用导致 UTF-8 字符边界 panic。
// 修复后：接口改成 (old_text, new_text, old_edit_range, new_edit_range)，
// expand_to_paragraph_boundaries 用 floor_char_boundary 保证 byte offset 是 char boundary。
// 这些测试验证修复后不再 panic，且返回正确结果。
#[cfg(test)]
mod issue_710_comment_5732160521_repro {
    use super::compute_affected_paragraph_ranges;

    /// 问题 1 — 插入场景不再 panic。
    ///
    /// old = "甲"（"甲"是 3 字节 UTF-8，占 byte 0..3）
    /// 在开头输入 ASCII "a"，new = "a甲"
    /// old_edit_range = (0, 0)（插入点在 old 文本的位置）
    /// new_edit_range = (0, 1)（inserted_range，new 坐标系）
    ///
    /// 修复前：pipeline 把 0..1 同时传给 old/new，old 侧 expand_to_paragraph_boundaries("甲",0,1)
    ///   执行 text[1..] 切到"甲"第二个字节 → panic。
    /// 修复后：old 侧用 (0,0)，new 侧用 (0,1)，且 floor_char_boundary 调整 offset，
    ///   不再 panic。
    #[test]
    fn test_issue710_insert_utf8_boundary_no_panic() {
        let result = std::panic::catch_unwind(|| {
            compute_affected_paragraph_ranges("甲", "a甲", (0, 0), (0, 1))
        });
        assert!(
            result.is_ok(),
            "修复后不应 panic：compute_affected_paragraph_ranges(\"甲\", \"a甲\", (0,0), (0,1)) \
             应正常返回，实际 panic: {}",
            result
                .as_ref()
                .err()
                .map(|p| {
                    p.downcast_ref::<&'static str>()
                        .map(|s| (*s).to_string())
                        .or_else(|| p.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "<non-string>".to_string())
                })
                .unwrap_or_default()
        );
        // 验证返回值正确：
        // old 侧 expand_to_paragraph_boundaries("甲", 0, 0) → (0, 3)（"甲"整个段落，3 bytes）
        // new 侧 expand_to_paragraph_boundaries("a甲", 0, 1) → (0, 4)（"a甲"整个段落，4 bytes）
        let (old_s, old_e, new_s, new_e) = result.unwrap();
        assert_eq!(
            (old_s, old_e),
            (0, 3),
            "old 侧插入点 (0,0) 在 \"甲\" 中扩展段落边界应为 (0,3)"
        );
        assert_eq!(
            (new_s, new_e),
            (0, 4),
            "new 侧 (0,1) 在 \"a甲\" 中扩展段落边界应为 (0,4)"
        );
    }

    /// 问题 1 — 删除场景不再 panic。
    ///
    /// old = "a甲"，删除开头 "a"，deleted_range = 0..1（old 坐标系），new = "甲"
    /// old_edit_range = (0, 1)（deleted_range，old 坐标系）
    /// new_edit_range = (0, 0)（删除后落点在 new 文本的位置）
    ///
    /// 修复前：pipeline 把 0..1 同时传给 old/new，new 侧 expand_to_paragraph_boundaries("甲",0,1)
    ///   执行 text[1..] 切到"甲"内部 → panic。
    /// 修复后：old 侧用 (0,1)，new 侧用 (0,0)，且 floor_char_boundary 调整 offset，
    ///   不再 panic。
    #[test]
    fn test_issue710_delete_utf8_boundary_no_panic() {
        let result = std::panic::catch_unwind(|| {
            compute_affected_paragraph_ranges("a甲", "甲", (0, 1), (0, 0))
        });
        assert!(
            result.is_ok(),
            "修复后不应 panic：compute_affected_paragraph_ranges(\"a甲\", \"甲\", (0,1), (0,0)) \
             应正常返回，实际 panic: {}",
            result
                .as_ref()
                .err()
                .map(|p| {
                    p.downcast_ref::<&'static str>()
                        .map(|s| (*s).to_string())
                        .or_else(|| p.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "<non-string>".to_string())
                })
                .unwrap_or_default()
        );
        // 验证返回值正确：
        // old 侧 expand_to_paragraph_boundaries("a甲", 0, 1) → (0, 4)（"a甲" 整个段落，4 bytes）
        // new 侧 expand_to_paragraph_boundaries("甲", 0, 0) → (0, 3)（"甲" 整个段落，3 bytes）
        let (old_s, old_e, new_s, new_e) = result.unwrap();
        assert_eq!(
            (old_s, old_e),
            (0, 4),
            "old 侧 (0,1) 在 \"a甲\" 中扩展段落边界应为 (0,4)"
        );
        assert_eq!(
            (new_s, new_e),
            (0, 3),
            "new 侧删除后落点 (0,0) 在 \"甲\" 中扩展段落边界应为 (0,3)"
        );
    }

    /// 问题 1 — 辅助：确认纯 ASCII 场景正常工作（对照测试）。
    #[test]
    fn test_issue710_ascii_control_no_panic() {
        let result = std::panic::catch_unwind(|| {
            compute_affected_paragraph_ranges("a", "ba", (0, 0), (0, 1))
        });
        assert!(
            result.is_ok(),
            "纯 ASCII 场景不应 panic，实际 panic: {}",
            result
                .as_ref()
                .err()
                .map(|p| {
                    p.downcast_ref::<&'static str>()
                        .map(|s| (*s).to_string())
                        .or_else(|| p.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "<non-string>".to_string())
                })
                .unwrap_or_default()
        );
    }

    /// 问题 1 — 验证 floor_char_boundary 在多字节字符内部 offset 时正确调整。
    ///
    /// old = "甲乙"（6 bytes，甲=0..3，乙=3..6）
    /// new_edit_range = (1, 4)（byte 1 落在"甲"内部，byte 4 落在"乙"内部）
    /// floor_char_boundary(1) = 0，floor_char_boundary(4) = 3
    /// 扩展后 new 侧段落边界 = (0, 6)（整个段落）
    #[test]
    fn test_issue710_floor_char_boundary_adjusts_mid_char_offset() {
        let (old_s, old_e, new_s, new_e) =
            compute_affected_paragraph_ranges("甲", "甲乙", (0, 0), (1, 4));
        // old 侧 (0,0) 在 "甲" 中扩展段落边界 → (0, 3)（"甲"整个段落）
        assert_eq!((old_s, old_e), (0, 3));
        // new 侧 floor_char_boundary(1)=0, floor_char_boundary(4)=3
        // expand_to_paragraph_boundaries("甲乙", 0, 3) → (0, 6)
        assert_eq!((new_s, new_e), (0, 6));
    }
}
