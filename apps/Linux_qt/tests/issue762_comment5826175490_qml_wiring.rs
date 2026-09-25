//! Issue #762 评论 5826175490 — 同步冲突状态全局入口的 QML 接线守卫。
//!
//! WHITE_BOX 验证策略：QML 无法在本仓库的 Rust 测试里实例化（
//! 组件依赖 `qml_resources` qrc 与 Rust 注册的上下文属性），因此按仓库既有惯例
//! （见 `repro_issue_687.rs` / `issue702_*`）读取 QML 源码，确定性断言这次修复的
//! 接线不变量，防止回退：
//!
//! 1. `WritingWorkspace.qml` 打开作品/切换作品/收到 `sync_conflicts_changed` 时都刷新
//!    冲突，且 `checkConflictsAfterSync` 不再依赖 `sync_operation_state` 的最终 status。
//! 2. `SyncPage.qml` 真的解析 `list_all_sync_conflicts()` 的 JSON、按作品分组、
//!    在状态区显示待处理冲突数、并发出 `openConflict(projectId, path)`。
//! 3. `main.qml` 收到 `openConflict` 后打开对应作品，并把目标路径交给
//!    `WritingWorkspace.openConflictPath()`——包含 Loader 尚未实例化时的补齐路径。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

/// 返回 apps/Linux_qt 根目录。
fn linux_qt_root() -> PathBuf {
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

/// 取从 `marker` 开始的最多 `len` 字节窗口，结束位置回退到最近的 UTF-8 边界。
fn window_from(src: &str, marker: &str, len: usize) -> String {
    let start = src
        .find(marker)
        .unwrap_or_else(|| panic!("marker `{marker}` must exist"));
    let target = (start + len).min(src.len());
    let end = (start..=target)
        .rev()
        .find(|i| src.is_char_boundary(*i))
        .unwrap_or(start);
    src[start..end].to_string()
}

/// 去掉整行 `//` 注释，只留可执行语句。
///
/// 守卫断言的是"代码不再依赖某模式"，注释里说明历史原因提到该模式不算违规
/// （例如 "不再依赖 sync_operation_state" 这句注释本身）。
fn strip_line_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| match line.find("//") {
            Some(idx) => &line[..idx],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ─────────────────────────────────────────────────────────────────────────
// 1. WritingWorkspace.qml
// ─────────────────────────────────────────────────────────────────────────

/// 打开作品时就刷新冲突——已有冲突不需要用户先手动同步一次才看得到。
#[test]
fn writing_workspace_refreshes_conflicts_on_open() {
    let src = read_src("qml/WritingWorkspace.qml");
    let on_completed = window_from(&src, "Component.onCompleted: {", 900);
    assert!(
        on_completed.contains("refreshConflictList()"),
        "Component 初始化完成后必须刷新冲突列表，实际窗口:\n{on_completed}"
    );
}

/// 切换作品时刷新冲突，并清掉属于上一个作品的选中路径。
#[test]
fn writing_workspace_refreshes_conflicts_on_project_change() {
    let src = read_src("qml/WritingWorkspace.qml");
    let handler = window_from(&src, "onWorkspaceProjectIdChanged:", 700);
    assert!(
        handler.contains("refreshConflictList()"),
        "workspaceProjectId 改变后必须刷新冲突列表，实际窗口:\n{handler}"
    );
    assert!(
        handler.contains("root.conflictPath = \"\""),
        "切换作品时必须清掉上一个作品的 conflictPath，避免误选中，实际窗口:\n{handler}"
    );
}

/// 收到 `sync_conflicts_changed` 就刷新冲突列表（同步过程中新冲突立即可见）。
#[test]
fn writing_workspace_listens_to_sync_conflicts_changed() {
    let src = read_src("qml/WritingWorkspace.qml");
    let connections = window_from(&src, "function onSync_conflicts_changed()", 300);
    assert!(
        connections.contains("refreshConflictList()"),
        "onSync_conflicts_changed 必须刷新冲突列表，实际窗口:\n{connections}"
    );
}

/// 「有没有冲突」不再依赖 `sync_operation_state` 的最终 status。
///
/// 冲突是持久状态，和当前有没有在跑一轮同步是两回事。回到读 status 就是回退。
#[test]
fn writing_workspace_does_not_gate_conflicts_on_sync_status() {
    let src = read_src("qml/WritingWorkspace.qml");
    let handler = strip_line_comments(&window_from(
        &src,
        "function checkConflictsAfterSync()",
        900,
    ));
    assert!(
        !handler.contains("sync_operation_state"),
        "checkConflictsAfterSync 不得再读 sync_operation_state 判断是否有冲突，\
         实际代码:\n{handler}"
    );
    assert!(
        !handler.contains("statusCode"),
        "checkConflictsAfterSync 不得再按 status 分支决定是否显示冲突，实际代码:\n{handler}"
    );
    assert!(
        handler.contains("refreshConflictList()"),
        "checkConflictsAfterSync 应直接刷新冲突列表，实际代码:\n{handler}"
    );
}

/// 提供显式的冲突选中入口，供 SyncPage 跨作品跳转使用。
#[test]
fn writing_workspace_exposes_open_conflict_path() {
    let src = read_src("qml/WritingWorkspace.qml");
    let func = window_from(&src, "function openConflictPath(path)", 700);
    assert!(
        func.contains("root.conflictPath = path"),
        "openConflictPath 必须记录目标冲突路径，实际窗口:\n{func}"
    );
    assert!(
        func.contains("refreshConflictList()"),
        "openConflictPath 必须刷新冲突列表，实际窗口:\n{func}"
    );
    assert!(
        func.contains("conflictTabIdx"),
        "openConflictPath 必须打开冲突 tab，实际窗口:\n{func}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 2. SyncPage.qml — 跨作品冲突入口
// ─────────────────────────────────────────────────────────────────────────

/// SyncPage 必须真正调用并解析全局冲突查询结果。
#[test]
fn sync_page_reads_global_conflicts_as_json() {
    let src = read_src("qml/SyncPage.qml");
    let refresh = window_from(&src, "function refreshAllSyncConflicts()", 1200);
    assert!(
        refresh.contains("list_all_sync_conflicts()"),
        "必须调用 Core 的全局冲突查询，实际窗口:\n{refresh}"
    );
    assert!(
        refresh.contains("JSON.parse(raw)"),
        "list_all_sync_conflicts() 返回 JSON 字符串，必须 JSON.parse 后再取字段，\
         实际窗口:\n{refresh}"
    );
    assert!(
        refresh.contains("resp.data.conflicts"),
        "必须从 ResultEnvelope.data.conflicts 取冲突数组，实际窗口:\n{refresh}"
    );
}

/// 按作品分组、展平成渲染行，并显示跨作品待处理冲突总数。
#[test]
fn sync_page_groups_conflicts_by_project() {
    let src = read_src("qml/SyncPage.qml");
    let group = window_from(&src, "function groupConflictsByProject(flat)", 1200);
    assert!(
        group.contains("entry.projectId"),
        "分组必须按 projectId 归并，实际窗口:\n{group}"
    );
    assert!(
        group.contains("entry.projectTitle"),
        "分组必须保留 Core 返回的作品标题，实际窗口:\n{group}"
    );

    let rows = window_from(&src, "function buildConflictRows(groups)", 1400);
    assert!(
        rows.contains("kind: \"project\""),
        "每组必须产出一个作品分组头行，实际窗口:\n{rows}"
    );
    assert!(
        rows.contains("kind: \"conflict\""),
        "每条冲突必须产出一个可点击行，实际窗口:\n{rows}"
    );

    assert!(
        src.contains("待处理冲突"),
        "同步状态区域必须显示待处理冲突数"
    );
    assert!(
        src.contains("Repeater") && src.contains("model: root.conflictRows"),
        "冲突列表必须按 conflictRows 渲染（分组头 + 每条冲突）"
    );
}

/// 点击某条冲突时发出 `openConflict(projectId, path)`。
#[test]
fn sync_page_emits_open_conflict() {
    let src = read_src("qml/SyncPage.qml");
    assert!(
        src.contains("signal openConflict(string projectId, string path)"),
        "SyncPage 必须声明 openConflict(projectId, path) 信号"
    );
    assert!(
        src.contains("root.openConflict(modelData.projectId, modelData.localPath)"),
        "点击冲突行必须以作品 id + 冲突路径发出 openConflict"
    );
}

/// 全局入口在冲突状态变化时刷新，不要求这一轮同步先结束。
#[test]
fn sync_page_refreshes_conflicts_on_signal() {
    let src = read_src("qml/SyncPage.qml");
    let handler = window_from(&src, "function onSync_conflicts_changed()", 300);
    assert!(
        handler.contains("refreshAllSyncConflicts()"),
        "onSync_conflicts_changed 必须刷新全局冲突入口，实际窗口:\n{handler}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 3. main.qml — openConflict 接线
// ─────────────────────────────────────────────────────────────────────────

/// 收到 `openConflict` 后打开对应作品，并把路径交给 WritingWorkspace 选中。
#[test]
fn main_qml_routes_open_conflict_to_workspace() {
    let src = read_src("qml/main.qml");
    assert!(
        src.contains("onOpenConflict: function(projectId, path)"),
        "main.qml 必须接住 SyncPage 的 openConflict 信号"
    );

    let route = window_from(&src, "function openConflictInProject(projectId, path)", 900);
    assert!(
        route.contains("appController.openWriting("),
        "openConflict 必须打开对应作品，实际窗口:\n{route}"
    );
    assert!(
        route.contains("window.pendingConflictPath = path"),
        "openConflict 必须记住待选中的冲突路径，实际窗口:\n{route}"
    );
    assert!(
        route.contains("applyPendingConflictPath()"),
        "openConflict 必须尝试立即应用待选中路径，实际窗口:\n{route}"
    );

    let apply = window_from(&src, "function applyPendingConflictPath()", 700);
    assert!(
        apply.contains("openConflictPath(pendingConflictPath)"),
        "待选中路径必须通过 WritingWorkspace.openConflictPath 交付，实际窗口:\n{apply}"
    );
    assert!(
        apply.contains("pendingConflictPath = \"\""),
        "交付成功后必须清掉待选中路径，避免下次打开误选中，实际窗口:\n{apply}"
    );
}

/// Loader 尚未实例化时（用户在 hub/设置页点冲突）也要能补交路径。
#[test]
fn main_qml_applies_pending_path_when_loader_loads() {
    let src = read_src("qml/main.qml");
    let loader = window_from(&src, "id: writingWorkspaceLoader", 500);
    assert!(
        loader.contains("onLoaded: window.applyPendingConflictPath()"),
        "writingWorkspaceLoader 加载完成后必须补交待选中的冲突路径，实际窗口:\n{loader}"
    );
}

/// 从全局冲突入口打开作品时带上真实作品标题，不把顶栏标题清成占位文字。
#[test]
fn main_qml_resolves_project_title_for_conflict_navigation() {
    let src = read_src("qml/main.qml");
    assert!(
        src.contains("function projectTitleById(projectId)"),
        "main.qml 必须能按 projectId 解析作品标题"
    );
    let route = window_from(&src, "function openConflictInProject(projectId, path)", 900);
    assert!(
        route.contains("window.projectTitleById(projectId)"),
        "openWriting 必须带上真实作品标题，实际窗口:\n{route}"
    );
}
