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
//! 4. 章节树：卷展开赋全新对象触发 model 重建；章节行发 `openChapter`；章纲显示当前
//!    章节真实 `chapter.note`，保存失败回滚编辑框而不是把本地缓存冒充成已保存。
//! 5. 宽屏设置两列、窄屏上下单列，两组列都在同一份 `settingsColumns` 里，
//!    不再用 `visible: root.widePanel` 隐藏右列丢掉三组设置。
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
// 5. WritingChapterNavigation.qml — 卷展开 / 章节点击 / 真实章纲
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

/// 章纲接真实 chapter.note，不再留空实现，并提供失败回滚入口。
#[test]
fn outline_shows_real_chapter_note() {
    let src = read_src("qml/WritingChapterNavigation.qml");
    assert!(
        src.contains("property string currentChapterNote: \"\""),
        "章节导航必须接收当前章节 note（唯一来源 editorController.chapterNote）"
    );
    assert!(
        src.contains("signal chapterNoteChanged(string note)"),
        "编辑完成必须把 note 交回 backend"
    );
    assert!(
        src.contains("onEditingFinished: root.chapterNoteChanged(outlineTextArea.text)"),
        "章纲编辑完成必须发 chapterNoteChanged"
    );

    let restore = slice_between(
        &src,
        "function restoreChapterNoteFromSource() {",
        "function volumesArray()",
    );
    assert!(
        restore.contains("outlineTextArea.text = root.currentChapterNote"),
        "失败回滚必须把编辑框恢复成 Core 当前值，实际窗口:\n{restore}"
    );

    let code = strip_line_comments(&src);
    assert!(
        !code.contains("还没接上"),
        "章纲不得再留\"还没接上\"的空实现"
    );
    assert!(
        src.contains("qsTr(\"请选择章节\")"),
        "未选章节时显示空态提示"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 6. WritingWorkspace.qml / EditorController.qml / backend — note 事务
// ─────────────────────────────────────────────────────────────────────────

/// 两处章纲处理都必须看后端 envelope，失败不更新本地缓存并回滚编辑框。
#[test]
fn chapter_note_save_failure_restores_core_value() {
    let src = read_src("qml/WritingWorkspace.qml");
    let code = strip_line_comments(&src);
    assert_eq!(
        code.matches("update_chapter_note(").count(),
        2,
        "Workbench 与 SinglePane 两处章纲都要写回 Core"
    );
    assert_eq!(
        code.matches("if (result && result.success)").count(),
        2,
        "两处都必须检查后端 envelope 的 success"
    );
    assert_eq!(
        code.matches("restoreChapterNoteFromSource()").count(),
        2,
        "两处失败路径都要恢复编辑框"
    );

    let workbench = slice_between(
        &src,
        "onChapterNoteChanged: function(note) {",
        "// Middle Area: Toolbar + Editor",
    );
    assert!(
        workbench.contains("editorController.chapterNote = note"),
        "成功路径才更新本地 chapterNote，实际窗口:\n{workbench}"
    );
    assert!(
        workbench.contains("sidebarRect.restoreChapterNoteFromSource()"),
        "失败路径必须回滚 Workbench 编辑框，实际窗口:\n{workbench}"
    );

    let single_pane = slice_from(&src, "id: singlePaneNavPanel");
    assert!(
        single_pane.contains("editorController.chapterNote = note"),
        "SinglePane 成功路径才更新本地 chapterNote，实际窗口:\n{single_pane}"
    );
    assert!(
        single_pane.contains("singlePaneNavPanel.restoreChapterNoteFromSource()"),
        "失败路径必须回滚 SinglePane 编辑框，实际窗口:\n{single_pane}"
    );
}

/// 打开章节写入真实 meta.note，清空章节同步清空缓存。
#[test]
fn editor_controller_tracks_chapter_note_from_core() {
    let src = read_src("qml/EditorController.qml");
    assert!(
        src.contains("property string chapterNote: \"\""),
        "EditorController 必须持有当前章节 note 状态"
    );

    let load = slice_between(
        &src,
        "// Issue #835：同步当前章节章纲（chapter.note）。",
        "// Return full result",
    );
    assert!(
        load.contains("result.data.meta.note"),
        "note 必须来自 Core 的 meta.note，实际窗口:\n{load}"
    );
    assert!(
        load.contains("controller.chapterNote = note"),
        "打开章节成功后写入 chapterNote，实际窗口:\n{load}"
    );

    let clear = slice_between(
        &src,
        "function clearActiveChapter()",
        "function reportStatsIfChanged",
    );
    assert!(
        clear.contains("chapterNote = \"\""),
        "清空章节时同步清空章纲缓存，实际窗口:\n{clear}"
    );
}

/// Linux_Qt backend 只做 Core 的薄封装，并注册成 QML 可调用方法。
#[test]
fn linux_qt_backend_exposes_core_update_chapter_note() {
    let ops = read_src("src/backend/chapter_operations.rs");
    let wrapper = slice_from(&ops, "pub(crate) fn update_chapter_note(");
    assert!(
        wrapper.contains("api.update_chapter_note(&p, &v, &c, &note_str)"),
        "薄封装必须直接调 Core，不在平台端存第二份 note，实际窗口:\n{wrapper}"
    );
    assert!(
        wrapper.contains("bridge_success_object"),
        "成功返回统一 envelope，实际窗口:\n{wrapper}"
    );
    assert!(
        wrapper.contains("bridge_error_object"),
        "失败也返回统一 envelope，实际窗口:\n{wrapper}"
    );

    let backend = read_src("src/backend/editor_backend.rs");
    let declaration = slice_between(
        &backend,
        "update_chapter_note: qt_method!(",
        "report_writing_event: qt_method!(",
    );
    assert!(
        declaration.contains("note: QString"),
        "QML 方法签名必须收四个参数（project/volume/chapter/note），实际窗口:\n{declaration}"
    );

    let implementation = slice_between(
        &backend,
        "fn update_chapter_note(",
        "fn report_writing_event(",
    );
    assert!(
        implementation.contains("app.update_chapter_note(project_id, volume_id, chapter_id, note)"),
        "backend 实现必须转发到 chapter_operations 的薄封装，实际窗口:\n{implementation}"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// 7. SettingsDialog.qml — 宽屏两列 / 窄屏上下单列
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
