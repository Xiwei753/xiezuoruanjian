use super::types::VisualLine;

// ── Qt 文本布局模块：VisualLine diff / affected paragraph ranges ──

/// Issue #658 评论 5626628570: old/new VisualLine 差异分类结果。
///
/// 把受影响的视觉行分成三类，避免 break 后尾部 while 把剩余全文加回 affected（Bug 1），
/// 并区分"需要重新栅格化"和"只是位置变化可复用纹理"两类行，使 y 变化的行能走
/// reflow_move 协同动画（Bug 2）。
#[derive(Clone, Debug)]
pub struct VisualLineDiff {
    /// 需要重新栅格化 QImage 的 old 行索引（内容/shaping/行内几何变化）。
    pub old_raster_line_ids: Vec<usize>,
    /// 需要重新栅格化 QImage 的 new 行索引（内容/shaping/行内几何变化）。
    pub new_raster_line_ids: Vec<usize>,
    /// 内容和 shaping 完全相同、只是 x/y 文档位置变化的 (old_idx, new_idx) 对。
    /// 不重新栅格化，复用 old 行已有 image/clusters/source rect，用 new VisualLine 的 x/y 生成终点。
    pub reusable_move_pairs: Vec<(usize, usize)>,
}

/// Issue #658 评论 5624570557 问题 2 / 评论 5626628570: 比较 old/new VisualLine，
/// 计算真正需要动画的行，并按"重新栅格化"与"复用纹理位移"分类。
///
/// 考虑换行/删换行时的受影响后续 reflow 行和相邻新旧段落。
/// 返回 [`VisualLineDiff`]，其中：
/// - `old_raster_line_ids` / `new_raster_line_ids`：内容/shaping/行内几何变化的行，需重新栅格化 QImage。
/// - `reusable_move_pairs`：内容和 shaping 完全相同、只是 x/y 文档位置变化的 (old, new) 对，
///   复用 old 行已有纹理，用 new VisualLine 的 x/y 生成 reflow_move 终点。
///
/// 算法：
/// 1. 找出字节范围相交的行（编辑点行）→ raster。
/// 2. 从编辑点行之后按 old/new byte offset 对应逐行比较：
///    - 内容/shaping 变化（width/height/qtextline_idx/字节长度）→ raster。
///    - 内容/shaping 相同、只是 x/y 位置变化 → reusable_move_pair。
///    - 完全相同（含 x/y）→ 稳定，立即停止扫描（修复 Bug 1：稳定后不再 append 剩余行）。
///    - 新增/消失的视觉行 → raster。
/// 3. 只有扫描走到某一侧末尾（未提前稳定停止）、另一侧确实还有未配对的新增/消失视觉行时，
///    才把那一侧真正未配对的尾巴加入。
/// 4. 考虑相邻段落首行缩进变化。
pub fn compare_old_new_visual_lines(
    old_lines: &[VisualLine],
    new_lines: &[VisualLine],
    inserted_range: Option<(usize, usize)>,
    deleted_range: Option<(usize, usize)>,
) -> VisualLineDiff {
    let mut old_raster_line_ids: Vec<usize> = Vec::new();
    let mut new_raster_line_ids: Vec<usize> = Vec::new();
    let mut reusable_move_pairs: Vec<(usize, usize)> = Vec::new();

    if old_lines.is_empty() || new_lines.is_empty() {
        return VisualLineDiff {
            old_raster_line_ids,
            new_raster_line_ids,
            reusable_move_pairs,
        };
    }

    // Issue #658 评论 5630181473 问题 2: old/new 两侧分别用各自的坐标系计算
    // affected range 和 downstream anchor，不再共享同一份 inserted_range。
    // replace / IME commit 同时有 inserted + deleted 时，old 侧按 deleted range
    // 找直接受影响行，new 侧按 inserted range 找直接受影响行。
    //
    // old_affected_*: old 坐标系中被修改的字节范围
    //   - 纯 insert: (ins_start, ins_start) — old 无字节变化，用插入点作 anchor
    //   - 纯 delete: deleted_range
    //   - replace: union(del_range, ins_range_in_old) — 包含被删字节和插入点
    //
    // new_affected_*: new 坐标系中被修改的字节范围
    //   - 纯 insert: inserted_range
    //   - 纯 delete: (del_start, del_start) — new 无字节变化，用删除点作 anchor
    //   - replace: inserted_range
    let (old_affected_start, old_affected_end) = match (inserted_range, deleted_range) {
        (Some((ins_start, ins_end)), Some((del_start, del_end))) => {
            // replace: old 坐标系 union(del_range, ins_range mapped to old)
            // delta = ins_size - del_size; ins in old = ins - delta
            let replace_delta =
                ((ins_end - ins_start) as isize - (del_end - del_start) as isize) as isize;
            let ins_start_old = ins_start.saturating_add_signed(-replace_delta);
            let ins_end_old = ins_end.saturating_add_signed(-replace_delta);
            (del_start.min(ins_start_old), del_end.max(ins_end_old))
        }
        (Some((_ins_start, _ins_end)), None) => {
            // pure insert: old 无字节变化，用插入点作为 hard-break/相邻行 anchor
            (_ins_start, _ins_start)
        }
        (None, Some((del_start, del_end))) => (del_start, del_end),
        (None, None) => (0, usize::MAX),
    };

    let (new_affected_start, new_affected_end) = match (inserted_range, deleted_range) {
        (Some((ins_start, ins_end)), Some((_del_start, _del_end))) => {
            // replace: new 坐标系用 inserted_range
            (ins_start, ins_end)
        }
        (Some((ins_start, ins_end)), None) => (ins_start, ins_end),
        (None, Some((del_start, _del_end))) => {
            // pure delete: new 无字节变化，用删除点作为 anchor
            (del_start, del_start)
        }
        (None, None) => (0, usize::MAX),
    };

    // 计算 old 侧受影响的行 → 必须重新栅格化。
    // Issue #658 评论 5632506204 问题 1: pure insert/delete 的 old range 是零长度 point (p,p)，
    // 半开区间交集公式 byte_end >= p && byte_start < p 在文首(p=0)和软换行边界会漏行。
    // 改为显式 point 收集：byte_start <= point && byte_end >= point，覆盖两侧。
    if old_affected_start == old_affected_end {
        let point = old_affected_start;
        for (idx, old_line) in old_lines.iter().enumerate() {
            if old_line.byte_start <= point && old_line.byte_end >= point {
                old_raster_line_ids.push(idx);
            }
        }
    } else {
        for (idx, old_line) in old_lines.iter().enumerate() {
            let intersects =
                old_line.byte_end >= old_affected_start && old_line.byte_start < old_affected_end;
            if intersects {
                old_raster_line_ids.push(idx);
            }
        }
    }
    // 计算 new 侧受影响的行 → 必须重新栅格化。
    if new_affected_start == new_affected_end {
        let point = new_affected_start;
        for (idx, new_line) in new_lines.iter().enumerate() {
            if new_line.byte_start <= point && new_line.byte_end >= point {
                new_raster_line_ids.push(idx);
            }
        }
    } else {
        for (idx, new_line) in new_lines.iter().enumerate() {
            let intersects =
                new_line.byte_end >= new_affected_start && new_line.byte_start < new_affected_end;
            if intersects {
                new_raster_line_ids.push(idx);
            }
        }
    }

    // 编辑点之后 old→new 的 byte offset 偏移
    let delta: isize = match (inserted_range, deleted_range) {
        (Some((ins_start, ins_end)), Some((del_start, del_end))) => {
            (ins_end - ins_start) as isize - (del_end - del_start) as isize
        }
        (Some((ins_start, ins_end)), None) => (ins_end - ins_start) as isize,
        (None, Some((del_start, del_end))) => -((del_end - del_start) as isize),
        (None, None) => 0,
    };

    // Downstream anchor: 完全在编辑区域之后的第一行索引。
    // old 侧用 old_affected_end（old 坐标系），new 侧用 new_affected_end（new 坐标系）。
    // 替代旧的 last_direct_affected_idx + 1 方案：当 raster 集合为空时（如段尾 \n，
    // 没有任何 VisualLine 与编辑字节严格相交），旧方案 fallback 到 len()，+1 后超出
    // 边界，扫描循环 while oi < old_lines.len() 永不执行，导致 reusable_move_pairs
    // 得不到任何下游 reflow 行。
    let old_downstream_anchor = old_lines
        .iter()
        .position(|l| l.byte_start >= old_affected_end)
        .unwrap_or(old_lines.len());
    let new_downstream_anchor = new_lines
        .iter()
        .position(|l| l.byte_start >= new_affected_end)
        .unwrap_or(new_lines.len());

    // 双指针逐行比较：从 downstream anchor 开始，按 old/new byte offset 对应。
    // 对齐的行分三种：
    //   - 内容/shaping 变化（width/height/qtextline_idx/字节长度）→ raster（重新栅格化）
    //   - 内容/shaping 完全相同、只是 x/y 文档位置变化 → reusable_move_pair（复用纹理）
    //   - 完全相同（含 x/y）→ 稳定，立即停止扫描
    // 未对齐的行（新增/消失的视觉行）→ raster。
    // 一旦稳定停止，绝对不再 append 剩余行（修复 Bug 1）。
    // 只有扫描走到一侧末尾、且另一侧确实还有未配对的新增/消失视觉行时，才把那一侧
    // 真正未配对的尾巴加入。
    let mut oi = old_downstream_anchor;
    let mut ni = new_downstream_anchor;
    let mut stable = false;
    while oi < old_lines.len() && ni < new_lines.len() {
        let ol = &old_lines[oi];
        let nl = &new_lines[ni];
        let corr_new_byte_start = ol.byte_start.saturating_add_signed(delta);
        if nl.byte_start < corr_new_byte_start {
            // new 这行是新增的视觉行
            new_raster_line_ids.push(ni);
            ni += 1;
            continue;
        }
        if nl.byte_start > corr_new_byte_start {
            // old 这行消失了
            old_raster_line_ids.push(oi);
            oi += 1;
            continue;
        }
        // byte_start 对齐，比较视觉内容
        // same_shape: 内容/shaping 相同（不含 x/y 文档位置）
        let same_shape = (ol.width - nl.width).abs() < 0.1
            && (ol.height - nl.height).abs() < 0.1
            && ol.qtextline_idx == nl.qtextline_idx
            && (ol.byte_end - ol.byte_start) == (nl.byte_end - nl.byte_start);
        // same_pos: x/y 文档位置相同（修复 Bug 2：原 same 不比较 y）
        let same_pos = (ol.x - nl.x).abs() < 0.1 && (ol.y - nl.y).abs() < 0.1;
        if same_shape && same_pos {
            // 完全相同，稳定，停止向后扩展
            stable = true;
            break;
        }
        if same_shape {
            // 内容/shaping 完全相同，只是 x/y 文档位置变化 → 复用纹理走 reflow_move
            // 修复点 2 (Issue #658 评论 5627327573): 确保互斥——已在 raster 集合的行
            // 不进 reusable_move_pairs，避免同一行同时进入 raster 和 reusable_move。
            if !old_raster_line_ids.contains(&oi) && !new_raster_line_ids.contains(&ni) {
                reusable_move_pairs.push((oi, ni));
            }
            oi += 1;
            ni += 1;
            continue;
        }
        // 内容/shaping/行内几何变化 → 重新栅格化
        old_raster_line_ids.push(oi);
        new_raster_line_ids.push(ni);
        oi += 1;
        ni += 1;
    }
    // 只有未提前稳定停止（扫描走到一侧末尾）时，才把另一侧剩余行加入。
    // 这些是真正新增/消失的视觉行（换行/删换行导致的尾部 reflow）。
    // 修复 Bug 1：稳定停止后绝对不再 append 剩余行。
    if !stable {
        while oi < old_lines.len() {
            old_raster_line_ids.push(oi);
            oi += 1;
        }
        while ni < new_lines.len() {
            new_raster_line_ids.push(ni);
            ni += 1;
        }
    }

    // 考虑相邻段落首行缩进变化（old 侧）
    for idx in 0..old_lines.len() {
        let old_line = &old_lines[idx];
        if old_line.qtextline_idx == 0 && old_raster_line_ids.contains(&idx) {
            if let Some(new_line) = new_lines
                .iter()
                .find(|l| l.para_start == old_line.para_start && l.qtextline_idx == 0)
            {
                if (new_line.x - old_line.x).abs() > 0.1 && !old_raster_line_ids.contains(&idx) {
                    old_raster_line_ids.push(idx);
                }
            }
        }
    }
    // 考虑新段落首行（old 中不存在的段落）
    for new_line in new_lines.iter() {
        if new_line.qtextline_idx == 0 {
            let is_new_para = !old_lines
                .iter()
                .any(|l| l.para_start == new_line.para_start);
            if is_new_para {
                if let Some(old_idx) = old_lines
                    .iter()
                    .position(|l| l.para_start == new_line.para_start)
                {
                    if !old_raster_line_ids.contains(&old_idx) {
                        old_raster_line_ids.push(old_idx);
                    }
                }
            }
        }
    }

    // 考虑相邻段落首行缩进变化（new 侧）
    for idx in 0..new_lines.len() {
        let new_line = &new_lines[idx];
        if new_line.qtextline_idx == 0 && new_raster_line_ids.contains(&idx) {
            if let Some(old_line) = old_lines
                .iter()
                .find(|l| l.para_start == new_line.para_start && l.qtextline_idx == 0)
            {
                if (new_line.x - old_line.x).abs() > 0.1 && !new_raster_line_ids.contains(&idx) {
                    new_raster_line_ids.push(idx);
                }
            }
        }
    }
    // 考虑新段落首行（new 侧：new 中存在但 old 中不存在的段落）
    for (idx, new_line) in new_lines.iter().enumerate() {
        if new_line.qtextline_idx == 0 {
            let is_new_para = !old_lines
                .iter()
                .any(|l| l.para_start == new_line.para_start);
            if is_new_para && !new_raster_line_ids.contains(&idx) {
                new_raster_line_ids.push(idx);
            }
        }
    }
    // 修复点 2 (Issue #658 评论 5627327573): 保证三个集合互斥——
    // 从 reusable_move_pairs 移除任何 old_idx ∈ old_raster_line_ids
    // 或 new_idx ∈ new_raster_line_ids 的对，避免同一行同时进入 raster 和 reusable_move。
    // 同时由 sort+dedup 去重。不在返回前把互斥留给 pipeline 猜。
    reusable_move_pairs
        .retain(|&(o, n)| !old_raster_line_ids.contains(&o) && !new_raster_line_ids.contains(&n));
    old_raster_line_ids.sort_unstable();
    old_raster_line_ids.dedup();
    new_raster_line_ids.sort_unstable();
    new_raster_line_ids.dedup();
    reusable_move_pairs.sort_unstable();
    reusable_move_pairs.dedup();

    VisualLineDiff {
        old_raster_line_ids,
        new_raster_line_ids,
        reusable_move_pairs,
    }
}

/// Issue #710 评论 5731145076 症状四/五: 根据old/new text和改动位置计算
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
