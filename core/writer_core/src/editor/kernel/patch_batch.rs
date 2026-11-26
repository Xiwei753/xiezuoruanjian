//! `DisplayPatch` 原子批次的唯一构造/核对入口（Issue #1063）。
//!
//! 契约（所有平台按同一份规则消费）：
//!
//! - 一个 [`EditorEditResult`](super::result::EditorEditResult) 里的
//!   `display_patches` 是**同一 base revision 的原子批次**，共享
//!   `base_revision` / `new_revision` / 结果选区。
//! - 每条 patch 的 `replace_byte_range` 都是 **base 文档 byte 坐标**；
//!   批次按 `replace_byte_range.start` **严格降序**输出，区间互不重叠。
//! - 同起点的多项变化在 Core 内合并成**一项**有明确定义的替换：组内按「后执行
//!   的插入落在更前面」的语义**逆序**拼接 inserted text，替换区间取同组最大 end。
//!   平台不得再自行排序或依赖稳定排序猜同起点顺序。
//! - `inserted_byte_range` 由 Core 从 base 坐标 + 各 patch 长度增量**推导**，
//!   是插入内容在最终 new revision 中的准确位置；平台不需要（也不能）用
//!   `replace_byte_range.start + inserted_text.len()` 去推。
//!
//! 平台侧（Qt / 鸿蒙）只需：校验 revision 一致、按给定顺序校验区间合法
//! （越界/字符边界/降序/不重叠），然后逐条应用。

use super::result::EditorEditResult;
use super::types::DisplayPatch;
use crate::editor::strong_types::{EditorRevision, Utf8ByteRange};
use crate::editor::transaction::EditorSelection;

/// 批次构造项：一次局部编辑在 **base 文档坐标**中的替换。
#[derive(Debug, Clone, Copy)]
pub(crate) struct PatchBatchItem<'a> {
    /// base 文档中被替换掉的区间（半开）。
    pub(crate) replace_range: Utf8ByteRange,
    /// 该区间在 base 文档中的真实 byte 长度（`delta.deleted_text.len()`）。
    ///
    /// 构造器用它核对「区间长度 == 实际删除文本长度」，保证 range 来自真实
    /// UTF-8 字符边界上的删除，而不是随手拼出来的数字。
    pub(crate) deleted_len: usize,
    /// 替换进最终文本的内容。
    pub(crate) inserted_text: &'a str,
}

/// 批次契约错误。正常编辑路径不应出现；测试用它核对不变量。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PatchBatchError {
    /// `start > end`。
    InvertedRange { start: usize, end: usize },
    /// 区间超出 base 文档长度。
    OutOfBounds { end: usize, base_len: usize },
    /// 区间长度与实际删除文本长度不一致。
    DeletedLenMismatch {
        start: usize,
        end: usize,
        deleted_len: usize,
    },
    /// 合并后仍出现重叠区间（调用方给出的编辑事务本身重叠）。
    Overlapping {
        previous_end: usize,
        next_start: usize,
    },
    /// 按降序表达的相邻两条 patch，顺序不严格递减。
    NotStrictlyDescending {
        previous_start: usize,
        next_start: usize,
    },
    /// 区间端点不在字符边界上。
    NotCharBoundary { offset: usize },
    /// `inserted_byte_range` 与从 base 坐标推导的最终坐标不一致。
    InsertedRangeMismatch {
        expected_start: usize,
        actual_start: usize,
    },
    /// `inserted_text.len()` 与 `inserted_byte_range` 长度不一致。
    InsertedLenMismatch { start: usize, end: usize },
}

/// 由单批局部编辑构造降序 `DisplayPatch` 批次。
///
/// `base_len` 是事务开始前正文的 byte 长度（所有 `replace_range` 都相对它）。
/// 调用方按**实际执行顺序**给出 items；同起点项在最终正文里的先后与执行顺序
/// 相反（后插入的落在更前面），构造器按该规则合并。
///
/// 前置条件由 [`TextEditDelta`](super::TextEditDelta) 的构造过程保证：区间来自
/// 已校验过 char boundary 的局部编辑。构造器仍会核对区间长度与删除文本长度、
/// 非重叠与边界（debug 构建下断言），并在发布构建里按同一算法确定性合并。
pub(crate) fn build_display_patches(
    base_revision: EditorRevision,
    new_revision: EditorRevision,
    selection: EditorSelection,
    base_len: usize,
    items: Vec<PatchBatchItem<'_>>,
) -> Vec<DisplayPatch> {
    let mut sorted = items;
    // 只按起点排序（stable）：同起点内部保持调用方给出的执行顺序，
    // 合并时再逆序拼接，得到最终正文里的真实先后。
    sorted.sort_by_key(|item| item.replace_range.start().value());

    // 合并同起点项：替换区间取组内最大 end，插入文本按组内逆序拼接。
    let mut groups: Vec<Vec<PatchBatchItem<'_>>> = Vec::new();
    let mut last_start: Option<usize> = None;
    for item in sorted {
        let start = item.replace_range.start().value();
        let end = item.replace_range.end().value();
        debug_assert!(
            validate_item(base_len, start, end, item.deleted_len).is_ok(),
            "DisplayPatch 批次项非法：start={start} end={end} deleted_len={} base_len={base_len}",
            item.deleted_len
        );
        if last_start == Some(start) {
            if let Some(group) = groups.last_mut() {
                group.push(item);
                continue;
            }
        }
        groups.push(vec![item]);
        last_start = Some(start);
    }
    let merged: Vec<(usize, usize, String)> = groups
        .into_iter()
        .map(|group| {
            let start = group[0].replace_range.start().value();
            let end = group
                .iter()
                .map(|item| item.replace_range.end().value())
                .max()
                .unwrap_or(start);
            let mut inserted_text = String::new();
            for item in group.iter().rev() {
                inserted_text.push_str(item.inserted_text);
            }
            let deleted_len: usize = group.iter().map(|item| item.deleted_len).sum();
            debug_assert_eq!(
                deleted_len,
                end - start,
                "DisplayPatch 同起点合并的删除长度与区间不一致：start={start} end={end}"
            );
            (start, end, inserted_text)
        })
        .collect();

    let selection_range = EditorEditResult::selection_byte_range(selection);
    let mut patches: Vec<DisplayPatch> = Vec::with_capacity(merged.len());
    // 升序遍历时累计长度增量，推出每条 patch 插入内容在最终正文中的位置。
    let mut cumulative_deleted: i128 = 0;
    let mut cumulative_inserted: i128 = 0;
    let mut previous_end = 0usize;
    for (start, end, inserted_text) in &merged {
        debug_assert!(
            *start >= previous_end,
            "DisplayPatch 批次区间重叠：previous_end={previous_end} next_start={start}"
        );
        previous_end = *end;

        #[allow(clippy::cast_sign_loss)]
        let inserted_start = (*start as i128 - cumulative_deleted + cumulative_inserted) as usize;
        cumulative_deleted += (*end - *start) as i128;
        cumulative_inserted += inserted_text.len() as i128;

        patches.push(DisplayPatch {
            base_revision,
            new_revision,
            replace_byte_range: Utf8ByteRange::from_ordered(*start, *end),
            inserted_byte_range: Utf8ByteRange::from_start_len(inserted_start, inserted_text.len()),
            inserted_text: inserted_text.clone(),
            resulting_selection_byte_range: selection_range,
        });
    }
    patches.reverse();

    debug_assert!(
        validate_display_patches(&patches, base_len).is_ok(),
        "DisplayPatch 批次不满足契约：{:?}",
        validate_display_patches(&patches, base_len)
    );
    patches
}

fn validate_item(
    base_len: usize,
    start: usize,
    end: usize,
    deleted_len: usize,
) -> Result<(), PatchBatchError> {
    if start > end {
        return Err(PatchBatchError::InvertedRange { start, end });
    }
    if end > base_len {
        return Err(PatchBatchError::OutOfBounds { end, base_len });
    }
    if end - start != deleted_len {
        return Err(PatchBatchError::DeletedLenMismatch {
            start,
            end,
            deleted_len,
        });
    }
    Ok(())
}

/// 核对一个 `DisplayPatch` 批次是否满足契约（降序、不重叠、坐标可推导）。
///
/// `base_len` 是批次 `base_revision` 对应正文的 byte 长度。测试与平台适配层
/// 的 Core 侧镜像校验使用；`is_char_boundary` 由调用方提供（Core 内部传 Rope
/// 判断，避免 materialize 全文）。
pub(crate) fn validate_display_patches(
    patches: &[DisplayPatch],
    base_len: usize,
) -> Result<(), PatchBatchError> {
    validate_display_patches_with_boundary(patches, base_len, |_| true)
}

/// 带 char boundary 谓词的批次核对（平台镜像层用真实 base 文本判断）。
pub(crate) fn validate_display_patches_with_boundary(
    patches: &[DisplayPatch],
    base_len: usize,
    is_char_boundary: impl Fn(usize) -> bool,
) -> Result<(), PatchBatchError> {
    // 降序：先整体校验顺序与不重叠，再按升序推导 inserted 坐标。
    let mut previous_start: Option<usize> = None;
    for patch in patches {
        let start = patch.replace_byte_range.start().value();
        let end = patch.replace_byte_range.end().value();
        if start > end {
            return Err(PatchBatchError::InvertedRange { start, end });
        }
        if end > base_len {
            return Err(PatchBatchError::OutOfBounds { end, base_len });
        }
        if !is_char_boundary(start) {
            return Err(PatchBatchError::NotCharBoundary { offset: start });
        }
        if !is_char_boundary(end) {
            return Err(PatchBatchError::NotCharBoundary { offset: end });
        }
        if let Some(prev_start) = previous_start {
            if start >= prev_start {
                return Err(PatchBatchError::NotStrictlyDescending {
                    previous_start: prev_start,
                    next_start: start,
                });
            }
        }
        previous_start = Some(start);
    }
    // 降序相邻：左侧（next，起点更小）patch 的 end 不得大于右侧（previous）的 start。
    for pair in patches.windows(2) {
        let right = &pair[0];
        let left = &pair[1];
        if left.replace_byte_range.end().value() > right.replace_byte_range.start().value() {
            return Err(PatchBatchError::Overlapping {
                previous_end: left.replace_byte_range.end().value(),
                next_start: right.replace_byte_range.start().value(),
            });
        }
    }

    let mut cumulative_deleted: i128 = 0;
    let mut cumulative_inserted: i128 = 0;
    for patch in patches.iter().rev() {
        let start = patch.replace_byte_range.start().value();
        let end = patch.replace_byte_range.end().value();
        #[allow(clippy::cast_sign_loss)]
        let expected_start = (start as i128 - cumulative_deleted + cumulative_inserted) as usize;
        let actual_start = patch.inserted_byte_range.start().value();
        if actual_start != expected_start {
            return Err(PatchBatchError::InsertedRangeMismatch {
                expected_start,
                actual_start,
            });
        }
        let inserted_end = patch.inserted_byte_range.end().value();
        if inserted_end != actual_start + patch.inserted_text.len() {
            return Err(PatchBatchError::InsertedLenMismatch {
                start: actual_start,
                end: inserted_end,
            });
        }
        cumulative_deleted += (end - start) as i128;
        cumulative_inserted += patch.inserted_text.len() as i128;
    }
    Ok(())
}

#[cfg(test)]
#[path = "patch_batch_tests.rs"]
mod tests;
