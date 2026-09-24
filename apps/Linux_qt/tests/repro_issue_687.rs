//! Issue #687 修复验证测试 — Linux Qt 协同吞吐动画已生效、删除光标不再回抽、写作区宽度设置已修复。
//!
//! WHITE_BOX 验证策略：通过读取项目源文件内容，确定性断言三个缺陷模式已在代码中消除。
//! 修复后，这些测试确认：
//! 1. Insert/Delete 分支不再传 &[], &[] 给 build_cluster_reflow_slices，而是显式排除 changed range
//! 2. build_cluster_reflow_slices 不再推断 InsertReveal/DeleteConceal（由显式函数拥有）
//! 3. cursor_controller apply_plan None 分支不再用 old_rect 作为 Tween 起点
//! 4. QML 不再使用 setting_linux_qt_editor_width，统一走 setting_desktop_editor_width

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

/// 返回 apps/Linux_qt 根目录。
fn linux_qt_root() -> PathBuf {
    // CARGO_MANIFEST_DIR 在 apps/Linux_qt 下，指向 apps/Linux_qt
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set by cargo");
    PathBuf::from(manifest_dir)
}

/// 读取指定相对路径源文件的完整内容。
fn read_src(rel: &str) -> String {
    let path = linux_qt_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
}

// ─────────────────────────────────────────────────────────────────────────
// 问题 1 修复验证：Insert/Delete 分支显式排除 changed range，
//         build_cluster_reflow_slices 不再推断 InsertReveal/DeleteConceal。
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn issue687_p1_insert_branch_excludes_inserted_range() {
    let src = read_src("src/sujian_editor_item/animation/transaction_builder.rs");
    // Insert 分支应调用 build_insert_reveal_slices 显式生成 InsertReveal
    assert!(
        src.contains("build_insert_reveal_slices("),
        "问题1修复: 应存在 build_insert_reveal_slices 显式函数"
    );
    // Insert 分支的 build_cluster_reflow_slices 调用应传入 inserted_range_tuple 作为 excluded_new_ranges
    let insert_call_idx = src
        .find("EditorAnimationKind::Insert =>")
        .expect("Insert branch must exist");
    let after_insert = &src[insert_call_idx..];
    let build_call_idx = after_insert
        .find("build_cluster_reflow_slices(")
        .expect("build_cluster_reflow_slices call must exist in Insert branch");
    let call_window = &after_insert[build_call_idx..build_call_idx + 500];
    // 应包含 inserted_range_tuple 作为 excluded_new_ranges（不再是 &[]）
    assert!(
        call_window.contains("inserted_range_tuple"),
        "问题1修复: Insert 分支应传入 inserted_range_tuple 排除 inserted_range。\
         调用窗口:\n{}",
        call_window
    );
    println!("[BUGFIX_687_VERIFY] P1 insert branch excludes inserted_range (FIXED)");
}

#[test]
fn issue687_p1_delete_branch_excludes_deleted_range() {
    let src = read_src("src/sujian_editor_item/animation/transaction_builder.rs");
    // Delete 分支应调用 build_delete_conceal_slices 显式生成 DeleteConceal
    assert!(
        src.contains("build_delete_conceal_slices("),
        "问题1修复: 应存在 build_delete_conceal_slices 显式函数"
    );
    // Delete 分支的 build_cluster_reflow_slices 调用应传入 deleted_ranges 作为 excluded_old_ranges
    let delete_call_idx = src
        .find("EditorAnimationKind::Delete =>")
        .expect("Delete branch must exist");
    let after_delete = &src[delete_call_idx..];
    let build_call_idx = after_delete
        .find("build_cluster_reflow_slices(")
        .expect("build_cluster_reflow_slices call must exist in Delete branch");
    let call_window = &after_delete[build_call_idx..build_call_idx + 500];
    // 应包含 deleted_ranges 作为 excluded_old_ranges（不再是 &[]）
    assert!(
        call_window.contains("&deleted_ranges"),
        "问题1修复: Delete 分支应传入 &deleted_ranges 排除 deleted_range。\
         调用窗口:\n{}",
        call_window
    );
    println!("[BUGFIX_687_VERIFY] P1 delete branch excludes deleted_range (FIXED)");
}

#[test]
fn issue687_p1_build_cluster_reflow_slices_no_longer_infers_insert_reveal() {
    let src = read_src("src/sujian_editor_item/animation/transaction_builder.rs");
    // build_cluster_reflow_slices 不再包含纯 new → insert_reveal 推断逻辑
    assert!(
        !src.contains("纯 new（无 old 对应）→ insert_reveal"),
        "问题1修复: build_cluster_reflow_slices 不应再包含 '纯 new → insert_reveal' 推断逻辑"
    );
    assert!(
        !src.contains("纯 old（无 new 对应）→ delete_conceal"),
        "问题1修复: build_cluster_reflow_slices 不应再包含 '纯 old → delete_conceal' 推断逻辑"
    );
    println!("[BUGFIX_687_VERIFY] P1 build_cluster_reflow_slices no longer infers InsertReveal/DeleteConceal (FIXED)");
}

// ─────────────────────────────────────────────────────────────────────────
// 问题 2 修复验证：cursor_controller apply_plan None 分支不再用 old_rect 作为 Tween 起点。
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn issue687_p2_apply_plan_none_branch_uses_visual_position() {
    let src = read_src("src/sujian_editor_item/cursor_controller.rs");
    // apply_plan 的 None 分支应使用 self.visible_x/self.visual_y 作为 Tween 起点
    // 而不是 old_rect.x/old_rect.top
    assert!(
        src.contains("Issue #687: animation == None 分支永远从当前屏幕帧继续"),
        "问题2修复: apply_plan None 分支应有 Issue #687 注释"
    );
    // 不应再包含 (start_x - prev_vx).abs() < 0.01 条件判断
    assert!(
        !src.contains("(start_x - prev_vx).abs() < 0.01"),
        "问题2修复: apply_plan None 分支不应再用 (start_x - prev_vx).abs() < 0.01 判断"
    );
    // 应包含 old_visible 作为判断条件（光标此前已可见时用 visual_x/visual_y）
    assert!(
        src.contains("if old_visible"),
        "问题2修复: apply_plan None 分支应用 old_visible 判断是否用 visual 位置"
    );
    // 不应再用覆盖后的 self.visible 作为判断条件
    assert!(
        !src.contains("if self.visible"),
        "问题2修复: apply_plan None 分支不应再用覆盖后的 self.visible 判断"
    );
    println!("[BUGFIX_687_VERIFY] P2 apply_plan None branch uses visual_x/visual_y (FIXED)");
}

// ─────────────────────────────────────────────────────────────────────────
// 问题 3 修复验证：QML 不再使用 setting_linux_qt_editor_width，
//         统一走 setting_desktop_editor_width。
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn issue687_p3_qml_uses_setting_desktop_editor_width() {
    let writing_workspace = read_src("qml/WritingWorkspace.qml");
    let top_toolbar = read_src("qml/TopWritingToolbar.qml");
    // WritingWorkspace.qml 中不应再使用 setting_linux_qt_editor_width
    let ws_count = writing_workspace
        .matches("setting_linux_qt_editor_width")
        .count();
    assert_eq!(
        ws_count, 0,
        "问题3修复: WritingWorkspace.qml 不应再使用 setting_linux_qt_editor_width，实际 {} 处",
        ws_count
    );
    // TopWritingToolbar.qml 中不应再使用 setting_linux_qt_editor_width
    let tb_count = top_toolbar.matches("setting_linux_qt_editor_width").count();
    assert_eq!(
        tb_count, 0,
        "问题3修复: TopWritingToolbar.qml 不应再使用 setting_linux_qt_editor_width，实际 {} 处",
        tb_count
    );
    // 应使用 setting_desktop_editor_width
    let ws_desktop = writing_workspace
        .matches("setting_desktop_editor_width")
        .count();
    assert!(
        ws_desktop >= 3,
        "问题3修复: WritingWorkspace.qml 应至少有 3 处 setting_desktop_editor_width，实际 {} 处",
        ws_desktop
    );
    let tb_desktop = top_toolbar.matches("setting_desktop_editor_width").count();
    assert!(
        tb_desktop >= 2,
        "问题3修复: TopWritingToolbar.qml 应至少有 2 处 setting_desktop_editor_width，实际 {} 处",
        tb_desktop
    );
    println!("[BUGFIX_687_VERIFY] P3 QML uses setting_desktop_editor_width (FIXED)");
}

#[test]
fn issue687_p3_rust_backend_only_has_setting_desktop_editor_width() {
    let settings_backend = read_src("src/backend/settings_backend.rs");
    // Rust 后端公开 setting_desktop_editor_width
    assert!(
        settings_backend.contains("setting_desktop_editor_width: qt_property!"),
        "Rust 后端应公开 setting_desktop_editor_width qt_property"
    );
    // Rust 后端不公开 setting_linux_qt_editor_width
    assert!(
        !settings_backend.contains("setting_linux_qt_editor_width"),
        "问题3: Rust 后端 settings_backend.rs 不应包含 setting_linux_qt_editor_width"
    );
    // 全局 src/ 确认无 setting_linux_qt_editor_width
    let src_dir = linux_qt_root().join("src");
    let mut found_in_src = Vec::new();
    for entry in walk_rust_files(&src_dir) {
        let content = std::fs::read_to_string(&entry).unwrap_or_default();
        if content.contains("setting_linux_qt_editor_width") {
            found_in_src.push(entry.display().to_string());
        }
    }
    assert!(
        found_in_src.is_empty(),
        "问题3: src/ 中不应存在 setting_linux_qt_editor_width，但在以下文件找到: {:?}",
        found_in_src
    );
    println!("[BUGFIX_687_VERIFY] P3 Rust backend only has setting_desktop_editor_width (FIXED)");
}

/// 递归收集目录下所有 .rs 文件路径。
fn walk_rust_files(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                result.extend(walk_rust_files(&path));
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                result.push(path);
            }
        }
    }
    result
}

// ─────────────────────────────────────────────────────────────────────────
// 综合验证测试：三个问题全部确认已修复
// ─────────────────────────────────────────────────────────────────────────

#[test]
fn issue687_all_three_problems_fixed() {
    issue687_p1_insert_branch_excludes_inserted_range();
    issue687_p1_delete_branch_excludes_deleted_range();
    issue687_p1_build_cluster_reflow_slices_no_longer_infers_insert_reveal();
    issue687_p2_apply_plan_none_branch_uses_visual_position();
    issue687_p3_qml_uses_setting_desktop_editor_width();
    issue687_p3_rust_backend_only_has_setting_desktop_editor_width();
    println!("[BUGFIX_687_VERIFY] Issue #687 all three problems FIXED");
}
