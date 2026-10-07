//! Issue #835 评论 6020436389 — 宽屏写作区 / 设置页 QML 接线守卫。
//!
//! WHITE_BOX 验证策略：QML 无法在本仓库的 Rust 测试里实例化（组件依赖
//! `qml_resources` qrc 与 Rust 注册的上下文属性），因此按仓库既有惯例
//! （见 `issue762_comment5826175490_qml_wiring.rs`）读取 QML 源码，确定性断言
//! #835 收口后的接线不变量，防止回退：
//!
//! 1. 右工具面板仍属于写作工作台。星图在 `RightDrawer` 内两态渲染（根星图列表 /
//!    嵌入 `StarMapWorkspace`），不再把 `openRootStarmap()` 转发到 `main.qml` 切
//!    `route`——那会连 `WritingWorkspace` 和正文未保存窗口一起释放。
//! 2. 最右 tool rail 保留手稿的 AI 入口（`enabled: false`），不靠删除入口规避重叠，
//!    也不打开一个空的 AI pane。
//! 3. 内容区四角色（章节栏 / 正文列 / 右工具面板 / 最右 rail）都填满 cross-axis 高度，
//!    命中区与视觉一致。
//! 4. 章节树：卷展开赋全新对象触发 model 重建；章节行发 `openChapter`；
//!    作品标题为静态行（没有折叠箭头、没有 projectGroupCollapsed）；
//!    不再有"章纲" TextArea / outlineGroupExpanded / chapterNoteChanged。
//! 5. 宽屏设置两列、窄屏上下单列，两组列都在同一份 `settingsColumns` 里，
//!    不再用 `visible: root.widePanel` 隐藏右列丢掉三组设置。
//! 6. 宽屏默认布局：layoutPlan 为 null 时默认宽屏侧栏（Side），只有 Core 明确
//!    返回 "Bottom" 才进入窄屏导航。
//!
//! 每条断言对应 #835 复核评论里明确要求或明确禁止的一条接线，不复制 QML 逻辑。

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

/// 去掉整行 `//` 注释，只留可执行语句。
///
/// 守卫断言的是"代码不再依赖某模式"，注释里说明历史原因提到该模式不算违规
/// （例如 "不再转发 openRootStarmap" 这句注释本身）。
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

/// 取 `start_marker`（含）到其后第一个 `end_marker`（不含）之间的源码。
///
/// 比固定字节窗口稳：注释长度变化不会把断言目标挤出窗口。
fn slice_between(src: &str, start_marker: &str, end_marker: &str) -> String {
    let start = src
        .find(start_marker)
        .unwrap_or_else(|| panic!("start marker `{start_marker}` must exist"));
    let rest = &src[start + start_marker.len()..];
    let end = rest
        .find(end_marker)
        .unwrap_or_else(|| panic!("end marker `{end_marker}` must exist after `{start_marker}`"));
    rest[..end].to_string()
}

/// 取 `start_marker`（含）到文件结尾之间的源码。
fn slice_from(src: &str, start_marker: &str) -> String {
    let start = src
        .find(start_marker)
        .unwrap_or_else(|| panic!("start marker `{start_marker}` must exist"));
    src[start..].to_string()
}

// ─────────────────────────────────────────────────────────────────────────
// 1. RightDrawer.qml — 星图在右栏内两态渲染，不切 route
// ─────────────────────────────────────────────────────────────────────────

/// 未选根星图显示真实列表，选中后内嵌真实 StarMapWorkspace / Canvas。
#[test]
fn right_drawer_embeds_real_starmap_inside_tool_pane() {
    let src = read_src("qml/RightDrawer.qml");
    assert!(
        src.contains("property string embeddedStarmapId: \"\""),
        "右栏必须有 embeddedStarmapId 本地状态区分两态"
    );
    assert!(
        src.contains("property string embeddedStarmapTitle: \"\""),
        "右栏必须有 embeddedStarmapTitle 本地状态区分两态"
    );

    // 未选根星图：右栏里显示真实 StarMapPage 列表（StarMapController.listStarmaps）。
    let list_state = slice_between(&src, "StarMapPage {", "// 选中根星图后");
    assert!(
        list_state.contains(
            "visible: root.selectedTool === \"starmap\" && root.embeddedStarmapId === \"\""
        ),
        "根星图列表只在未选根星图时可见，实际窗口:\n{list_state}"
    );
    assert!(
        list_state.contains("starMapController: root.starMapControllerRef"),
        "StarMapPage 必须拿真实 StarMapController 读根星图，实际窗口:\n{list_state}"
    );
    assert!(
        list_state.contains("onOpenStarmap: function(starmapId, title) {"),
        "点根星图必须切到右栏嵌入态，实际窗口:\n{list_state}"
    );
    assert!(
        list_state.contains("root.embeddedStarmapId = starmapId"),
        "点根星图必须只改右栏本地状态，实际窗口:\n{list_state}"
    );
    assert!(
        list_state.contains("root.embeddedStarmapTitle = title"),
        "点根星图必须记录标题，实际窗口:\n{list_state}"
    );

    // 选中根星图：仍在右栏内，复用真实 StarMapWorkspace / Canvas。
    let embed_state = slice_between(
        &src,
        "StarMapWorkspace {",
        "// 统计 — 复用现有 StatsPreviewPage",
    );
    assert!(
        embed_state.contains(
            "visible: root.selectedTool === \"starmap\" && root.embeddedStarmapId !== \"\""
        ),
        "嵌入 Workspace 只在选中根星图时可见，实际窗口:\n{embed_state}"
    );
    assert!(
        embed_state.contains("starmapBackendRef: root.starmapBackendRef"),
        "嵌入 Workspace 必须拿真实 starmap 后端，实际窗口:\n{embed_state}"
    );
    assert!(
        embed_state.contains("starmapId: root.embeddedStarmapId"),
        "嵌入 Workspace 必须跟随右栏选中的根星图，实际窗口:\n{embed_state}"
    );
    assert!(
        embed_state.contains("starmapTitle: root.embeddedStarmapTitle"),
        "嵌入 Workspace 必须拿到根星图标题，实际窗口:\n{embed_state}"
    );
    assert!(
        embed_state.contains("onBackClicked: {"),
        "嵌入态必须处理层级返回，实际窗口:\n{embed_state}"
    );
    assert!(
        embed_state.contains("root.embeddedStarmapId = \"\""),
        "返回只退回右栏根星图列表，实际窗口:\n{embed_state}"
    );
    assert!(
        embed_state.contains("root.embeddedStarmapTitle = \"\""),
        "返回必须清掉嵌入标题，实际窗口:\n{embed_state}"
    );
}

/// 右栏不得再出现跳出写作页的导航转发，也不得伪造 AI 占位内容。
#[test]
fn right_drawer_does_not_leave_writing_route() {
    let src = read_src("qml/RightDrawer.qml");
    let code = strip_line_comments(&src);
    assert!(
        !code.contains("openStarmapRequested"),
        "RightDrawer 不得再暴露 openStarmapRequested 转发信号"
    );
    assert!(
        !code.contains("openStarmapWorkspace"),
        "RightDrawer 不得再转发 openStarmapWorkspace"
    );
    assert!(
        !code.contains("openRootStarmap"),
        "右栏星图不得转发到 appController.openRootStarmap 切 route"
    );
    assert!(
        !code.contains("selectedTool === \"ai\""),
        "RightDrawer 不得为 AI 造占位内容"
    );
    assert!(
        !code.contains("AI 功能开发中"),
        "RightDrawer 不得保留 AI 开发中占位"
    );
    assert!(
        !code.contains("敬请期待"),
        "RightDrawer 不得保留敬请期待占位"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 2. WritingWorkspace.qml / main.qml — 透传真实星图引用，不转发导航
// ─────────────────────────────────────────────────────────────────────────

/// 写作区只把真实 controller/backend 透传给右栏，不再有跳页信号。
#[test]
fn writing_workspace_forwards_starmap_refs_without_route_forward() {
    let src = read_src("qml/WritingWorkspace.qml");
    let code = strip_line_comments(&src);
    assert!(
        !code.contains("openStarmapWorkspace"),
        "WritingWorkspace 不得再保留 openStarmapWorkspace 信号/转发"
    );
    assert!(
        !code.contains("openRootStarmap"),
        "WritingWorkspace 不得再调 appController.openRootStarmap"
    );

    let drawer = slice_between(&src, "RightDrawer {", "// Issue #825：最右竖向工具 rail");
    assert!(
        drawer.contains("starMapControllerRef: root.starMapControllerRef"),
        "RightDrawer 必须拿到真实 StarMapController（根星图列表），实际窗口:\n{drawer}"
    );
    assert!(
        drawer.contains("starmapBackendRef: root.starmapBackendRef"),
        "RightDrawer 必须拿到真实 starmap 后端（嵌入 Workspace/Canvas），实际窗口:\n{drawer}"
    );
    assert!(
        !drawer.contains("onOpenStarmapRequested"),
        "RightDrawer 实例不得再接线 openStarmapRequested，实际窗口:\n{drawer}"
    );
}

/// main.qml 的 WritingWorkspace 实例不再有 onOpenStarmapWorkspace 转发链。
#[test]
fn main_qml_writing_workspace_does_not_forward_starmap() {
    let src = read_src("qml/main.qml");
    let block = slice_between(
        &src,
        "id: writingWorkspaceLoader",
        "// EmptyWorkspace: loaded only when no workspace",
    );
    assert!(
        !block.contains("onOpenStarmapWorkspace"),
        "WritingWorkspace 实例不得再带 onOpenStarmapWorkspace 转发，实际窗口:\n{block}"
    );
    assert!(
        !block.contains("openRootStarmap"),
        "WritingWorkspace 实例不得再调 openRootStarmap 切 route，实际窗口:\n{block}"
    );
    assert!(
        block.contains("starMapControllerRef: globalStarMapController"),
        "main.qml 必须注入真实 StarMapController，实际窗口:\n{block}"
    );
    assert!(
        block.contains("starmapBackendRef: starmapBackend"),
        "main.qml 必须注入真实 starmap 后端，实际窗口:\n{block}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 3. WritingToolRail.qml — AI 入口保留在自己的 rail 位置，禁用而非删除
// ─────────────────────────────────────────────────────────────────────────

/// AI 入口仍占独立 44px 行，降低 opacity，且不发 toolRequested。
#[test]
fn tool_rail_keeps_disabled_ai_slot() {
    let src = read_src("qml/WritingToolRail.qml");
    let code = strip_line_comments(&src);
    assert!(
        code.contains("{ key: \"ai\", label: qsTr(\"AI\"), enabled: false }"),
        "rail 必须保留手稿的 AI 入口并标记 enabled:false"
    );

    let delegate = slice_between(&src, "Repeater {", "Item { Layout.fillHeight: true }");
    assert!(
        delegate.contains("model: root.tools"),
        "rail 入口必须直接渲染 tools 常驻数组，不得按 enabled 过滤，实际窗口:\n{delegate}"
    );
    assert!(
        delegate.contains("Layout.preferredHeight: 44"),
        "disabled 的 AI 入口仍要占自己的 44px 行，实际窗口:\n{delegate}"
    );
    assert!(
        delegate.contains("opacity: modelData.enabled !== false ? 1.0 : 0.4"),
        "disabled 入口要降文字 opacity 但不消失，实际窗口:\n{delegate}"
    );
    assert!(
        delegate.contains("enabled: modelData.enabled !== false"),
        "MouseArea.enabled 必须跟随 modelData.enabled，实际窗口:\n{delegate}"
    );
    assert!(
        delegate.contains("onClicked: root.toolRequested(modelData.key)"),
        "点击仍只发 toolRequested(key)，实际窗口:\n{delegate}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 4. WritingWorkspace.qml — 内容区四角色填满高度
// ─────────────────────────────────────────────────────────────────────────

/// 四个内容区角色都必须 Layout.fillHeight，命中区与视觉一致。
#[test]
fn content_area_roles_fill_cross_axis_height() {
    let src = read_src("qml/WritingWorkspace.qml");

    let chapter_nav = slice_between(
        &src,
        "WritingChapterNavigation {",
        "// Middle Area: Toolbar + Editor",
    );
    assert!(
        chapter_nav.contains("Layout.fillHeight: true"),
        "左章节栏必须填满内容区高度，否则 MouseArea 命中区不在视觉位置，实际窗口:\n{chapter_nav}"
    );

    let editor_column = slice_between(&src, "// Middle Area: Toolbar + Editor", "RightDrawer {");
    assert!(
        editor_column.contains("Layout.fillHeight: true"),
        "正文列必须填满内容区高度，实际窗口:\n{editor_column}"
    );

    let drawer = slice_between(&src, "RightDrawer {", "// Issue #825：最右竖向工具 rail");
    assert!(
        drawer.contains("Layout.fillHeight: true"),
        "右工具面板必须填满内容区高度，实际窗口:\n{drawer}"
    );

    let rail = slice_between(&src, "WritingToolRail {", "Connections {");
    assert!(
        rail.contains("Layout.fillHeight: true"),
        "最右 rail 必须占满内容区高度，实际窗口:\n{rail}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 5. WritingChapterNavigation.qml — 卷展开 / 章节点击 / 左树语义纠正
// ─────────────────────────────────────────────────────────────────────────

/// 卷展开必须整体赋新对象，QML 才会把 property var 视为变化并重建 model。
#[test]
fn volume_expand_rebuilds_tree_model_via_new_object() {
    let src = read_src("qml/WritingChapterNavigation.qml");
    let toggle = slice_between(
        &src,
        "function toggleVolumeExpanded(volumeId) {",
        "function volumesArray()",
    );
    assert!(
        toggle.contains("var next = {}"),
        "切换卷展开必须先建新对象，实际窗口:\n{toggle}"
    );
    assert!(
        toggle.contains("root.volumeExpandedMap = next"),
        "必须把全新对象整体赋回，onVolumeExpandedMapChanged 才会触发，实际窗口:\n{toggle}"
    );
    assert!(
        !toggle.contains("root.volumeExpandedMap[volumeId] ="),
        "不得原地改 JS 对象成员（QML property var 不会发 changed 信号），实际窗口:\n{toggle}"
    );
    assert!(
        src.contains("onVolumeExpandedMapChanged: root.buildTreeModel()"),
        "展开态变化必须重建 treeModel"
    );
}

/// 章节行左键必须直接进入真实 openChapter 链路。
#[test]
fn chapter_rows_emit_open_chapter() {
    let src = read_src("qml/WritingChapterNavigation.qml");
    let chapter_rows = slice_between(&src, "// ── 章节行 ──", "// Tree context menu");
    assert!(
        chapter_rows.contains(
            "root.openChapter(model.itemProjectId || root.workspaceProjectId, \
             model.itemVolumeId, model.itemId, model.itemTitle)"
        ),
        "章节行左键必须发 openChapter(projectId, volumeId, chapterId, title)，实际窗口:\n{chapter_rows}"
    );
}

/// Issue #836：左树语义纠正 — 作品标题为静态行，不再有折叠分组头。
#[test]
fn left_tree_project_title_is_static_no_collapse() {
    let src = read_src("qml/WritingChapterNavigation.qml");
    let code = strip_line_comments(&src);

    // 不得再有 projectGroupCollapsed / outlineGroupExpanded 属性
    assert!(
        !code.contains("projectGroupCollapsed"),
        "Issue #836：不得再有 projectGroupCollapsed 属性"
    );
    assert!(
        !code.contains("outlineGroupExpanded"),
        "Issue #836：不得再有 outlineGroupExpanded 属性"
    );

    // 不得再有 toggleProjectGroup / toggleOutlineGroup 信号
    assert!(
        !code.contains("toggleProjectGroup"),
        "Issue #836：不得再有 toggleProjectGroup 信号"
    );
    assert!(
        !code.contains("toggleOutlineGroup"),
        "Issue #836：不得再有 toggleOutlineGroup 信号"
    );

    // 不得再有 chapterNoteChanged 信号
    assert!(
        !code.contains("chapterNoteChanged"),
        "Issue #836：不得再有 chapterNoteChanged 信号"
    );

    // 不得再有 currentChapterNote 属性
    assert!(
        !code.contains("currentChapterNote"),
        "Issue #836：不得再有 currentChapterNote 属性"
    );

    // 不得再有 restoreChapterNoteFromSource 函数
    assert!(
        !code.contains("restoreChapterNoteFromSource"),
        "Issue #836：不得再有 restoreChapterNoteFromSource 函数"
    );

    // 不得再有 "章纲" TextArea
    assert!(
        !code.contains("outlineTextArea"),
        "Issue #836：不得再有章纲 TextArea"
    );

    // 不得再有 "请选择章节" 空态
    assert!(
        !code.contains("请选择章节"),
        "Issue #836：不得再有章纲空态提示"
    );

    // 作品标题必须是静态行（Rectangle + AppText），不是 WritingTreeGroupHeader
    // 查找顶部标题区域：在 ColumnLayout 里，作品标题行不应使用 WritingTreeGroupHeader
    let top_section = slice_between(
        &src,
        "ColumnLayout {",
        "// Tree list",
    );
    assert!(
        !top_section.contains("WritingTreeGroupHeader"),
        "Issue #836：顶部作品标题不得使用 WritingTreeGroupHeader（应为静态行），实际窗口:\n{top_section}"
    );
    assert!(
        top_section.contains("workspaceProjectTitle"),
        "Issue #836：顶部必须显示 workspaceProjectTitle，实际窗口:\n{top_section}"
    );

    // 卷仍走 WritingTreeGroupHeader
    let volume_header = slice_between(
        &src,
        "// ── 卷分组头 ──",
        "// 卷头右键菜单",
    );
    assert!(
        volume_header.contains("WritingTreeGroupHeader"),
        "Issue #836：卷仍必须走 WritingTreeGroupHeader，实际窗口:\n{volume_header}"
    );
}

/// Issue #836：WritingWorkspace 不得再传递 projectGroupCollapsed / outlineGroupExpanded / currentChapterNote。
#[test]
fn writing_workspace_no_group_collapse_props() {
    let src = read_src("qml/WritingWorkspace.qml");
    let code = strip_line_comments(&src);

    assert!(
        !code.contains("projectGroupCollapsed"),
        "Issue #836：WritingWorkspace 不得再有 projectGroupCollapsed"
    );
    assert!(
        !code.contains("outlineGroupExpanded"),
        "Issue #836：WritingWorkspace 不得再有 outlineGroupExpanded"
    );
    assert!(
        !code.contains("currentChapterNote"),
        "Issue #836：WritingWorkspace 不得再有 currentChapterNote"
    );
    assert!(
        !code.contains("chapterNote"),
        "Issue #836：WritingWorkspace 不得再有 chapterNote 相关代码"
    );
    assert!(
        !code.contains("update_chapter_note"),
        "Issue #836：WritingWorkspace 不得再调用 update_chapter_note"
    );
    assert!(
        !code.contains("restoreChapterNoteFromSource"),
        "Issue #836：WritingWorkspace 不得再调用 restoreChapterNoteFromSource"
    );
    assert!(
        !code.contains("toggleProjectGroup"),
        "Issue #836：WritingWorkspace 不得再有 toggleProjectGroup handler"
    );
    assert!(
        !code.contains("toggleOutlineGroup"),
        "Issue #836：WritingWorkspace 不得再有 toggleOutlineGroup handler"
    );
}

/// Issue #836：EditorController 不得再有 chapterNote 缓存属性。
#[test]
fn editor_controller_no_chapter_note() {
    let src = read_src("qml/EditorController.qml");
    let code = strip_line_comments(&src);

    assert!(
        !code.contains("chapterNote"),
        "Issue #836：EditorController 不得再有 chapterNote 属性或赋值"
    );
}

/// Issue #836：Linux_Qt backend 不再暴露 update_chapter_note。
#[test]
fn linux_qt_backend_no_update_chapter_note() {
    let ops = read_src("src/backend/chapter_operations.rs");
    let code = strip_line_comments(&ops);
    assert!(
        !code.contains("update_chapter_note"),
        "Issue #836：chapter_operations.rs 不得再有 update_chapter_note 方法"
    );

    let backend = read_src("src/backend/editor_backend.rs");
    let code = strip_line_comments(&backend);
    assert!(
        !code.contains("update_chapter_note"),
        "Issue #836：editor_backend.rs 不得再有 update_chapter_note 声明或实现"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 6. SettingsDialog.qml — 宽屏两列 / 窄屏上下单列
// ─────────────────────────────────────────────────────────────────────────

/// 两列都常驻：宽屏左右摆，窄屏上下接，不靠隐藏右列模拟单列。
#[test]
fn settings_wide_two_columns_narrow_stacked() {
    let src = read_src("qml/SettingsDialog.qml");

    let columns = slice_between(&src, "id: settingsColumns", "// Issue #695");
    assert!(
        columns.contains("root.widePanel ? (width - gap) / 2 : width"),
        "宽屏两列均分、窄屏占满整行，实际窗口:\n{columns}"
    );
    assert!(
        columns.contains(
            "Math.max(leftSettingsColumn.implicitHeight, rightSettingsColumn.implicitHeight)"
        ),
        "宽屏高度取两列较大者，实际窗口:\n{columns}"
    );
    assert!(
        columns.contains(
            "leftSettingsColumn.implicitHeight + gap + rightSettingsColumn.implicitHeight"
        ),
        "窄屏高度是两列上下相加，右列内容不能丢，实际窗口:\n{columns}"
    );
    assert!(columns.contains("id: leftSettingsColumn"), "必须保留左列");
    assert!(columns.contains("id: rightSettingsColumn"), "必须保留右列");
    assert!(
        columns.contains("x: root.widePanel ? leftSettingsColumn.width + settingsColumns.gap : 0"),
        "宽屏右列摆在右侧，窄屏归零换行，实际窗口:\n{columns}"
    );
    assert!(
        columns.contains(
            "y: root.widePanel ? 0 : leftSettingsColumn.implicitHeight + settingsColumns.gap"
        ),
        "窄屏右列接在左列下方，实际窗口:\n{columns}"
    );
    assert!(
        !columns.contains("visible: root.widePanel"),
        "不得用 visible: root.widePanel 隐藏右列，否则窄屏丢三组设置，实际窗口:\n{columns}"
    );

    let left = slice_between(&src, "id: leftSettingsColumn", "id: rightSettingsColumn");
    assert!(
        left.contains("qsTr(\"外观\")")
            && left.contains("qsTr(\"编辑器和动画\")")
            && left.contains("qsTr(\"AI\")"),
        "左列必须是 外观 / 编辑器和动画 / AI，实际窗口:\n{left}"
    );

    let right = slice_between(&src, "id: rightSettingsColumn", "// Issue #695");
    assert!(
        right.contains("qsTr(\"保存和同步\")")
            && right.contains("qsTr(\"诊断与日志\")")
            && right.contains("qsTr(\"关于\")"),
        "右列必须是 保存和同步 / 诊断与日志 / 关于，实际窗口:\n{right}"
    );

    assert!(
        src.contains("contentHeight: settingsColumns.height"),
        "ScrollView 内容高度必须跟随两列合成高度"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 7. 宽屏默认布局 — layoutPlan 为 null 时默认宽屏
// ─────────────────────────────────────────────────────────────────────────

/// Issue #836：CreativeHub.qml 的 wideShell 在 layoutPlan 为 null 时默认 true。
#[test]
fn creative_hub_wide_shell_defaults_to_side_when_plan_null() {
    let src = read_src("qml/CreativeHub.qml");
    assert!(
        src.contains("layoutPlan === null || layoutPlan.primaryNavigationPlacement === \"Side\""),
        "Issue #836：CreativeHub wideShell 必须在 layoutPlan===null 时默认 Side，实际:\n{src}"
    );

    // 确保不再用旧逻辑（layoutPlan && ...）
    let code = strip_line_comments(&src);
    assert!(
        !code.contains("layoutPlan && layoutPlan.primaryNavigationPlacement"),
        "Issue #836：CreativeHub 不得再使用旧逻辑 layoutPlan && ..."
    );
}

/// Issue #836：WritingWorkspace.qml 的 wideWorkbench 在 layoutPlan 为 null 时默认 true。
#[test]
fn writing_workspace_wide_workbench_defaults_when_plan_null() {
    let src = read_src("qml/WritingWorkspace.qml");
    assert!(
        src.contains("layoutPlan === null || layoutPlan.workspaceLayoutMode === \"Workbench\""),
        "Issue #836：WritingWorkspace wideWorkbench 必须在 layoutPlan===null 时默认 Workbench，实际:\n{src}"
    );

    let code = strip_line_comments(&src);
    assert!(
        !code.contains("layoutPlan && layoutPlan.workspaceLayoutMode"),
        "Issue #836：WritingWorkspace 不得再使用旧逻辑 layoutPlan && ..."
    );
}

/// Issue #836：SettingsDialog.qml 的 widePanel / overlayPanel 在 layoutPlan 为 null 时默认宽屏。
#[test]
fn settings_dialog_wide_defaults_when_plan_null() {
    let src = read_src("qml/SettingsDialog.qml");
    assert!(
        src.contains("layoutPlan === null || layoutPlan.primaryNavigationPlacement === \"Side\""),
        "Issue #836：SettingsDialog widePanel 必须在 layoutPlan===null 时默认 Side"
    );
    assert!(
        src.contains("layoutPlan === null || layoutPlan.workspaceLayoutMode === \"Workbench\""),
        "Issue #836：SettingsDialog overlayPanel 必须在 layoutPlan===null 时默认 Workbench"
    );

    let code = strip_line_comments(&src);
    assert!(
        !code.contains("layoutPlan && layoutPlan.primaryNavigationPlacement"),
        "Issue #836：SettingsDialog 不得再使用旧逻辑 layoutPlan && ..."
    );
    assert!(
        !code.contains("layoutPlan && layoutPlan.workspaceLayoutMode"),
        "Issue #836：SettingsDialog 不得再使用旧逻辑 layoutPlan && ..."
    );
}

/// Issue #836：只有 Core 明确返回 "Bottom" 才进入窄屏导航。
/// 验证三处 QML 的 wideShell/wideWorkbench/widePanel 都只在 "Bottom" 时为 false。
#[test]
fn only_bottom_placement_enters_narrow_shell() {
    // CreativeHub: wideShell = layoutPlan === null || placement === "Side"
    // → 只有 placement === "Bottom" 时 wideShell 才为 false
    let hub = read_src("qml/CreativeHub.qml");
    let hub_line = hub
        .lines()
        .find(|l| l.contains("wideShell:"))
        .expect("CreativeHub must have wideShell property");
    assert!(
        hub_line.contains("layoutPlan === null") && hub_line.contains("\"Side\""),
        "CreativeHub wideShell 必须在 null 时默认宽屏，只有 Bottom 才窄屏，实际:\n{hub_line}"
    );

    // WritingWorkspace: wideWorkbench = layoutPlan === null || mode === "Workbench"
    let ws = read_src("qml/WritingWorkspace.qml");
    let ws_line = ws
        .lines()
        .find(|l| l.contains("wideWorkbench:"))
        .expect("WritingWorkspace must have wideWorkbench property");
    assert!(
        ws_line.contains("layoutPlan === null") && ws_line.contains("\"Workbench\""),
        "WritingWorkspace wideWorkbench 必须在 null 时默认宽屏，实际:\n{ws_line}"
    );

    // SettingsDialog: widePanel = layoutPlan === null || placement === "Side"
    let sd = read_src("qml/SettingsDialog.qml");
    let sd_line = sd
        .lines()
        .find(|l| l.contains("widePanel:"))
        .expect("SettingsDialog must have widePanel property");
    assert!(
        sd_line.contains("layoutPlan === null") && sd_line.contains("\"Side\""),
        "SettingsDialog widePanel 必须在 null 时默认宽屏，实际:\n{sd_line}"
    );
}
