//! `patch_batch` 的单元测试（从生产文件拆出，避免内嵌测试模块超限）。

use super::*;

fn apply_descending(base: &str, patches: &[DisplayPatch]) -> String {
    let mut text = base.to_string();
    for patch in patches {
        let start = patch.replace_byte_range.start().value();
        let end = patch.replace_byte_range.end().value();
        text.replace_range(start..end, &patch.inserted_text);
    }
    text
}

fn item(start: usize, end: usize, inserted_text: &'static str) -> PatchBatchItem<'static> {
    PatchBatchItem {
        replace_range: Utf8ByteRange::from_ordered(start, end),
        deleted_len: end - start,
        inserted_text,
    }
}

fn revision(value: u64) -> EditorRevision {
    let mut revision = EditorRevision::initial();
    for _ in 0..value {
        revision = revision.next();
    }
    revision
}

fn selection() -> EditorSelection {
    crate::editor::kernel::result::make_selection(0, 0)
}

#[test]
fn multiple_edits_are_emitted_strictly_descending() {
    let base = "abcdef";
    let patches = build_display_patches(
        revision(0),
        revision(1),
        selection(),
        base.len(),
        vec![item(1, 3, "XY"), item(4, 5, "Z")],
    );
    assert_eq!(patches.len(), 2);
    assert_eq!(patches[0].replace_byte_range.start().value(), 4);
    assert_eq!(patches[1].replace_byte_range.start().value(), 1);
    assert_eq!(apply_descending(base, &patches), "aXYdZf");
    assert!(validate_display_patches_with_boundary(&patches, base.len(), |_| true).is_ok());
}

#[test]
fn same_start_items_merge_into_one_replacement() {
    let base = "abcdef";
    // 执行顺序：先删 [2,4)，再在 2 插入 X，再在 2 插入 Y。
    // 后插入的落在更前面，最终正文是 "abYXef"。
    let patches = build_display_patches(
        revision(0),
        revision(1),
        selection(),
        base.len(),
        vec![item(2, 4, ""), item(2, 2, "X"), item(2, 2, "Y")],
    );
    assert_eq!(patches.len(), 1);
    assert_eq!(patches[0].replace_byte_range.start().value(), 2);
    assert_eq!(patches[0].replace_byte_range.end().value(), 4);
    assert_eq!(patches[0].inserted_text, "YX");
    assert_eq!(apply_descending(base, &patches), "abYXef");
}

#[test]
fn same_start_point_inserts_land_before_range_replacement() {
    let base = "abcdef";
    // 先替换 [2,4) 为 "R"，再在同一 base 起点 2 插入 "P"：
    // 插入内容落在替换文本之前，合并为 replace [2,4) -> "PR"。
    let patches = build_display_patches(
        revision(0),
        revision(1),
        selection(),
        base.len(),
        vec![item(2, 4, "R"), item(2, 2, "P")],
    );
    assert_eq!(patches.len(), 1);
    assert_eq!(patches[0].inserted_text, "PR");
    assert_eq!(apply_descending(base, &patches), "abPRef");
}

#[test]
fn inserted_ranges_are_derived_in_final_coordinates() {
    // 替换变长后，后面 patch 的 inserted 坐标必须右移。
    let base = "abcdef";
    let patches = build_display_patches(
        revision(0),
        revision(1),
        selection(),
        base.len(),
        vec![item(1, 2, "LONGER"), item(4, 5, "!")],
    );
    assert_eq!(patches[0].replace_byte_range.start().value(), 4);
    assert_eq!(patches[0].inserted_byte_range.start().value(), 9);
    assert_eq!(patches[1].inserted_byte_range.start().value(), 1);
    assert_eq!(patches[1].inserted_byte_range.end().value(), 7);
    assert_eq!(apply_descending(base, &patches), "aLONGERcd!f");
}
