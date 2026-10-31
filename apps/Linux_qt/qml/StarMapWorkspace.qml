// =============================================================================
// StarMapWorkspace.qml — 星图工作区
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 页面）
// 职责：星图编辑工作区，只承载唯一一块星图画布
//
// Issue #805 评论 5907045450 第 1 部分：删掉"下钻换整页 graph"的模型。
//
// Issue #929：独立画布协议。
//   Workspace 只创建一块 StarMapCanvas（它管理"当前活动画布"的身份、镜头和
//   返回入口）。子星图不是递归塞进父图的坐标空间，也不是工作台维护的另一层
//   页面：进入 / 退出都由 Canvas 走 Core 导航路由完成，Workspace 只按
//   `canvas.focusIsRoot` 决定返回动作是"退出星图工作区"还是"返回父星图"。
//   工作台不持有任何子 Scene / 子 Content 引用，也不把焦点栈当画布身份。
//
// Issue #822：节点标题编辑已经移进节点自身的内联编辑，删掉 inspector 链路。
//
// Issue #977：删除本页自绘的 56px 顶栏和四边 border；返回 / 标题 / 公共入口
//   统一由 main.qml 的 DesktopTopBar 负责。Workspace 保留导航语义，暴露
//   focusIsRoot 与 navigateBack() 给壳层调用。
//
// Issue #1014：本页使用全应用统一的三栏骨架。
//   - 左栏：StarMapNavigationTree，宽度固定跟随 layoutState 全局值（默认 240），
//     不提供本页 SplitView 拖动入口；
//   - 中栏：唯一 StarMapCanvas；
//   - 右栏：StarMapToolDrawer + 56vp StarMapToolRail 作为一个完整容器，
//     总宽度 = layoutState.paneWidth("starmap.workspace", "right")；
//     折叠 / 展开整个容器，不再按窗口宽度临时算 260~340 的抽屉宽度。
//   嵌入模式（RightDrawer 内）保持无侧栏，避免第二套导航。
//
// 约束：
//   - 纯 UI 层，业务逻辑通过 StarMapCanvas 和各画布的 StarMapGraphController 委托
//   - 保留原有属性和信号，供 main.qml / 其他入口引用
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Item {
    id: root
    width: parent ? parent.width : 800
    height: parent ? parent.height : 600

    property string starmapId: ""
    property string starmapTitle: qsTr("星图")
    required property var dt
    property var starmapBackendRef: null
    // Issue #790 评论 5875963057: 公共入口收口后的应用状态。
    property var appState: ({})
    property var annotationBackendRef: null
    property var searchBackendRef: null
    property bool aiAvailable: false
    property bool aiEnabled: false
    // Issue #1016：写作与星图共用同一个 Agent CLI / 会话后端。
    property var agentCliBackendRef: null
    property var agentRuntimeBackendRef: null
    property string workspaceProjectId: ""
    property string workspaceProjectTitle: ""
    property bool embedded: false
    property string selectedTool: "tags"
    property bool toolDrawerOpen: true
    property var pendingLinkSource: null
    property string pendingLinkId: ""
    property var pendingLinkTarget: null
    property string pendingName: ""

    // Issue #1014：由 main.qml 注入的统一页面骨架状态。
    property var layoutState: null
    property string pageKey: "starmap.workspace"
    property bool fallbackLeftPaneCollapsed: false
    property bool fallbackRightPaneCollapsed: false

    readonly property bool leftPaneCollapsed: {
        if (root.embedded)
            return true
        return root.layoutState
            ? root.layoutState.isCollapsed(root.pageKey, "left")
            : root.fallbackLeftPaneCollapsed
    }
    readonly property bool rightPaneCollapsed: {
        if (root.embedded)
            return true
        return root.layoutState
            ? root.layoutState.isCollapsed(root.pageKey, "right")
            : root.fallbackRightPaneCollapsed
    }

    readonly property int toolRailWidth: 56
    readonly property int splitHandleWidth: 6
    readonly property int leftPaneMinWidth: 200
    // 左栏只跟随全局骨架宽度；本页不再提供自由拖拽。
    readonly property int leftPaneWidth: root.layoutState
        ? root.layoutState.paneWidth(root.pageKey, "left")
        : root.leftPaneMinWidth
    // Issue #1016：右栏总最小宽度 = 内容 200 + 工具轨 56。
    readonly property int rightPaneContentMinWidth: 200
    readonly property int rightPaneMinWidth: rightPaneContentMinWidth + toolRailWidth
    readonly property int rightPaneMaxWidth: 520
    readonly property int canvasMinWidth: 240

    // Issue #977：顶栏需要知道当前是根画布还是子画布，以决定返回文案与动作。
    readonly property bool focusIsRoot: canvas.focusIsRoot

    signal backClicked()
    signal requestSync()
    signal requestSearch()
    signal openSettings()
    signal sceneGeometryReady(string starmapId, string routeKey)
    signal annotationTargetRequested(var target)
    signal annotationToolRequested(string toolKey)

    property var pendingTreeRow: null

    onStarmapIdChanged: {
        if (!navigationTree)
            return
        navigationTree.currentObjectKey = ""
        navigationTree.reload()
    }

    // Issue #1016：把本次 SplitView 的真实宽度/可见状态上报给顶栏。
    function reportRenderedPane(side, pane) {
        if (!root.layoutState || !pane)
            return
        root.layoutState.setRenderedPaneWidth(root.pageKey, side,
            pane.visible ? pane.width : 0)
    }

    function setPaneCollapsed(side, collapsed) {
        if (root.layoutState) {
            root.layoutState.setCollapsed(root.pageKey, side, collapsed)
        } else if (side === "left") {
            root.fallbackLeftPaneCollapsed = collapsed
        } else {
            root.fallbackRightPaneCollapsed = collapsed
        }
    }

    function toggleLeftPane() {
        root.setPaneCollapsed("left", !root.leftPaneCollapsed)
    }

    function toggleRightPane() {
        if (root.rightPaneCollapsed) {
            root.toolDrawerOpen = true
            root.setPaneCollapsed("right", false)
        } else {
            root.toolDrawerOpen = false
            root.setPaneCollapsed("right", true)
        }
    }

    function closeRightPane() {
        root.toolDrawerOpen = false
        root.setPaneCollapsed("right", true)
    }

    function requestToolPane(toolKey) {
        root.selectedTool = toolKey || "tags"
        root.toolDrawerOpen = true
        root.setPaneCollapsed("right", false)
    }

    // ── 星图树回调 ──
    function backendEnvelope(raw) {
        try {
            return JSON.parse(String(raw || "{}"))
        } catch (e) {
            return null
        }
    }

    function backendSucceeded(raw) {
        var envelope = backendEnvelope(raw)
        return !!(envelope && envelope.success === true)
    }

    function projectIdForStarmap(id) {
        if (starmapBackendRef && id && starmapBackendRef.get_starmap_json) {
            try {
                var envelope = JSON.parse(String(starmapBackendRef.get_starmap_json(id) || "{}"))
                var meta = envelope && envelope.success === true ? envelope.data : null
                if (meta && meta.projectId) return String(meta.projectId)
            } catch (e) {}
        }
        var selected = root.appState && root.appState.selected ? root.appState.selected : ({})
        return String(selected.projectId || "")
    }

    readonly property string annotationProjectId: projectIdForStarmap(canvas.currentStarmapId || root.starmapId)
    readonly property var annotationTargetRef: {
        var projectId = root.annotationProjectId
        var sceneId = String(canvas.currentStarmapId || root.starmapId || "")
        if (!projectId || !sceneId) return null
        if (sharedSelectionController.scenePathKey === canvas.routeKey) {
            var selectedKind = sharedSelectionController.kind
            if (selectedKind === "node" && sharedSelectionController.itemId)
                return { kind: "starMapNode", projectId: projectId, objectId: sharedSelectionController.itemId }
            if (selectedKind === "embed" && sharedSelectionController.itemId)
                return { kind: "starMapEmbed", projectId: projectId, objectId: sharedSelectionController.itemId }
        }
        return { kind: "starMap", projectId: projectId, objectId: sceneId }
    }

    // Issue #984：对壳层暴露星图节点编辑的离开前保存入口。
    // 搜索跳章节/根图等会让 Loader 失活的导航必须先确认这里返回 true。
    function flushPendingNodeEdit() {
        return canvas.flushPendingNodeEdit()
    }

    // Issue #977：壳层统一返回入口。根画布退出星图工作区；子画布走 Core
    // 导航路由返回父画布，并恢复进入父画布时保存的镜头。
    // Issue #984：根画布退出前先提交节点编辑；失败保留工作区和草稿。
    function navigateBack() {
        if (canvas.focusIsRoot) {
            if (!canvas.flushPendingNodeEdit())
                return false
            root.backClicked()
            return true
        }
        return canvas.focusParentScene()
    }

    // Issue #982：全局搜索点击星图结果时定位到具体节点 / 边 / 嵌入。
    function revealSearchTarget(target) {
        return canvas.revealSearchTarget(target)
    }

    function syncTreeRoute(rootStarmapId, segmentsJson, routeKey) {
        if (!navigationTree)
            return
        // 画布真正换路由时，旧的普通节点高亮已经不属于当前画布。
        if (String(routeKey || "") !== String(navigationTree.currentRouteKey || ""))
            navigationTree.currentObjectKey = ""
        navigationTree.syncRoute(rootStarmapId, segmentsJson, routeKey)
    }

    function activateTreeRoute(rootStarmapId, segmentsJson, routeKey) {
        if (!canvas.navigateToTreeRoute(rootStarmapId, segmentsJson, routeKey)) {
            navigationTree.syncRoute(canvas.currentRoute.rootStarmapId,
                                     canvas.segmentsJsonOf(canvas.currentRoute),
                                     canvas.routeKey)
        }
    }

    // 普通节点行：先进入它所在的画布，再命中、选中并居中该对象。
    function revealTreeObject(rootStarmapId, segmentsJson, kind, objectId, objectKey) {
        var revealed = canvas.revealTreeObject(rootStarmapId, segmentsJson, kind, objectId)
        if (revealed) {
            navigationTree.currentObjectKey = String(objectKey || "")
            return true
        }
        navigationTree.currentObjectKey = ""
        navigationTree.syncRoute(canvas.currentRoute.rootStarmapId,
                                 canvas.segmentsJsonOf(canvas.currentRoute),
                                 canvas.routeKey)
        return false
    }

    function createObject(mode) {
        if (mode !== "node" && mode !== "starmap")
            return
        canvas.openCreateDialog(mode)
    }

    function renameTreeRow(rowInfo, title) {
        if (!rowInfo || !root.starmapBackendRef)
            return
        var trimmed = String(title || "").trim()
        if (!trimmed)
            return
        var raw = ""
        if (rowInfo.rowKind === "root") {
            raw = root.starmapBackendRef.rename_starmap_json(rowInfo.starmapId, trimmed)
        } else if (rowInfo.rowKind === "embed") {
            raw = root.starmapBackendRef.update_starmap_embed_json(
                        rowInfo.parentStarmapId, rowInfo.instanceId,
                        JSON.stringify({ label: trimmed, expectedRevision: rowInfo.revision }))
        } else if (rowInfo.rowKind === "portal" || rowInfo.rowKind === "node") {
            raw = root.starmapBackendRef.update_starmap_node_json(
                        rowInfo.parentStarmapId, rowInfo.nodeId,
                        JSON.stringify({ title: trimmed, expectedRevision: rowInfo.revision }))
        }
        if (!root.backendSucceeded(raw))
            return
        navigationTree.reload()
    }

    function deleteTreeRow(rowInfo) {
        if (!rowInfo || !root.starmapBackendRef)
            return
        var raw = ""
        if (rowInfo.rowKind === "root") {
            raw = root.starmapBackendRef.delete_starmap_json(rowInfo.starmapId)
        } else if (rowInfo.rowKind === "embed") {
            raw = root.starmapBackendRef.delete_starmap_embed_json(
                        rowInfo.parentStarmapId, rowInfo.instanceId,
                        Number(rowInfo.revision || 0))
        } else if (rowInfo.rowKind === "portal" || rowInfo.rowKind === "node") {
            raw = root.starmapBackendRef.delete_starmap_node_json(
                        rowInfo.parentStarmapId, rowInfo.nodeId,
                        Number(rowInfo.revision || 0))
        }
        if (!root.backendSucceeded(raw))
            return

        // 普通节点不是可导航画布：删除它只需要刷新树，不能按 routeKey
        // 把当前画布错误地导航到父级。
        if (rowInfo.rowKind === "node") {
            if (navigationTree.currentObjectKey === String(rowInfo.objectKey || ""))
                navigationTree.currentObjectKey = ""
            navigationTree.reload()
            return
        }
        if (canvas.routeKey === rowInfo.routeKey) {
            if (rowInfo.rowKind === "root") {
                root.backClicked()
                return
            }
            var parentSegments = []
            try {
                parentSegments = JSON.parse(String(rowInfo.routeSegments || "[]"))
                if (Array.isArray(parentSegments))
                    parentSegments = parentSegments.slice(0, Math.max(0, parentSegments.length - 1))
                else
                    parentSegments = []
            } catch (e) {
                parentSegments = []
            }
            canvas.navigateToTreeRoute(rowInfo.rootStarmapId, JSON.stringify(parentSegments), "")
        }
        navigationTree.reload()
    }

    function beginRenameTreeRow(rowInfo) {
        root.pendingTreeRow = rowInfo
        treeRenameField.text = rowInfo ? String(rowInfo.title || "") : ""
        treeRenameDialog.open()
    }

    function beginDeleteTreeRow(rowInfo) {
        root.pendingTreeRow = rowInfo
        treeDeleteDialog.open()
    }

    // Issue #814 评论 5935285879: 当前活动画布共享的唯一选中状态控制器。
    // 由 Workspace 创建，传给 Canvas；切换活动画布时选中身份带新的 routeKey。
    StarMapSelectionController {
        id: sharedSelectionController
    }

    Rectangle {
        anchors.fill: parent
        color: Kirigami.Theme.backgroundColor

        SplitView {
            id: paneSplitView
            anchors.fill: parent
            orientation: Qt.Horizontal

            property real resizeStartRightWidth: 0

            handle: Rectangle {
                id: handleDelegate
                implicitWidth: root.splitHandleWidth
                containmentMask: Item {
                    x: (handleDelegate.width - width) / 2
                    width: 14
                    height: paneSplitView.height
                }
                color: SplitHandle.pressed
                       ? Kirigami.Theme.highlightColor
                       : (SplitHandle.hovered
                          ? Kirigami.Theme.focusColor
                          : "transparent")
                Rectangle {
                    anchors.centerIn: parent
                    width: 1
                    height: parent.height
                    color: Kirigami.Theme.disabledTextColor
                    opacity: 0.3
                }
            }

            onResizingChanged: {
                if (paneSplitView.resizing) {
                    paneSplitView.resizeStartRightWidth = rightPaneContainer.width
                    return
                }
                if (!root.layoutState)
                    return
                // 左栏固定跟随全局 paneWidth，只有右栏拖动结果写回持久化。
                var rightChanged = Math.abs(rightPaneContainer.width
                                            - paneSplitView.resizeStartRightWidth) > 0.5
                if (rightChanged && rightPaneContainer.visible)
                    root.layoutState.setDraggedWidth(root.pageKey, "right",
                                                     rightPaneContainer.width)
            }

            // ── 左栏：星图层级树 ──
            Rectangle {
                id: leftPaneContainer
                visible: !root.embedded && !root.leftPaneCollapsed
                // 左栏宽度完全由全局骨架决定；min = max 让本页 SplitView
                // 不再提供可拖动的自由宽度入口。
                SplitView.minimumWidth: root.leftPaneWidth
                SplitView.preferredWidth: root.leftPaneWidth
                SplitView.maximumWidth: root.leftPaneWidth
                onWidthChanged: root.reportRenderedPane("left", leftPaneContainer)
                onVisibleChanged: root.reportRenderedPane("left", leftPaneContainer)
                color: "transparent"
                clip: true

                StarMapNavigationTree {
                    id: navigationTree
                    anchors.fill: parent
                    dt: root.dt
                    starmapBackendRef: root.starmapBackendRef
                    workspaceRootStarmapId: root.starmapId
                    onRouteActivated: function(rootStarmapId, segmentsJson, routeKey) {
                        root.activateTreeRoute(rootStarmapId, segmentsJson, routeKey)
                    }
                    onObjectActivated: function(rootStarmapId, segmentsJson,
                                                kind, objectId, objectKey) {
                        root.revealTreeObject(rootStarmapId, segmentsJson,
                                              kind, objectId, objectKey)
                    }
                    onCreateObjectRequested: function(mode) {
                        root.createObject(mode)
                    }
                    onRenameRequested: function(rowInfo) {
                        root.beginRenameTreeRow(rowInfo)
                    }
                    onDeleteRequested: function(rowInfo) {
                        root.beginDeleteTreeRow(rowInfo)
                    }
                }
            }

            // ── 中栏：唯一一块星图画布 ──
            StarMapCanvas {
                id: canvas
                SplitView.fillWidth: true
                SplitView.minimumWidth: root.canvasMinWidth
                dt: root.dt
                onSceneGeometryReady: function(starmapId, routeKey) {
                    root.sceneGeometryReady(starmapId, routeKey)
                }
                onRouteChanged: function(rootStarmapId, segmentsJson, routeKey) {
                    root.syncTreeRoute(rootStarmapId, segmentsJson, routeKey)
                }
                onTreeStructureChanged: navigationTree.reload()
                canvasBackgroundColor: Kirigami.Theme.backgroundColor
                starmapId: root.starmapId
                starmapBackendRef: root.starmapBackendRef
                annotationProjectId: root.annotationProjectId
                selectionController: sharedSelectionController
                onAnnotationActionRequested: function(action, targetRef, title) {
                    root.pendingLinkSource = targetRef
                    root.selectedTool = "tags"
                    if (action === "link") {
                        root.pendingLinkId = ""
                        root.pendingLinkTarget = null
                        annotationLinkPicker.open()
                    } else if (action === "registerName") {
                        var itemName = String(title || targetRef.objectId)
                        root.pendingName = itemName
                        registerNameDialog.open()
                    } else if (root.embedded) {
                        root.annotationToolRequested("tags")
                    } else {
                        root.requestToolPane("tags")
                    }
                }

                onVisibleChanged: {
                    if (!visible)
                        canvas.resetInteraction()
                }
            }

            // ── 右栏：工具内容 + 56vp rail 作为一个整体 ──
            Rectangle {
                id: rightPaneContainer
                visible: !root.embedded && !root.rightPaneCollapsed
                SplitView.minimumWidth: root.rightPaneMinWidth
                SplitView.preferredWidth: root.layoutState
                    ? root.layoutState.paneWidth(root.pageKey, "right")
                    : root.rightPaneMinWidth
                SplitView.maximumWidth: root.rightPaneMaxWidth
                onWidthChanged: root.reportRenderedPane("right", rightPaneContainer)
                onVisibleChanged: root.reportRenderedPane("right", rightPaneContainer)
                color: "transparent"
                clip: true

                RowLayout {
                    anchors.fill: parent
                    spacing: 0

                    StarMapToolDrawer {
                        id: starMapTools
                        visible: root.toolDrawerOpen
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        dt: root.dt
                        selectedTool: root.selectedTool
                        isOpen: visible
                        aiAvailable: root.aiAvailable
                        aiEnabled: root.aiEnabled
                        agentCliBackendRef: root.agentCliBackendRef
                        agentRuntimeBackendRef: root.agentRuntimeBackendRef
                        // 绑定作品的星图与写作页共用同一作品会话；未绑定时
                        // 以星图身份作为上下文 key，历史仍由 Core 保存。
                        workspaceProjectId: root.annotationProjectId !== ""
                            ? root.annotationProjectId : root.workspaceProjectId
                        workspaceProjectTitle: root.workspaceProjectTitle !== ""
                            ? root.workspaceProjectTitle : root.starmapTitle
                        aiContextLabel: root.starmapTitle
                        annotationBackendRef: root.annotationBackendRef
                        searchBackendRef: root.searchBackendRef
                        projectId: root.annotationProjectId
                        targetRef: root.annotationTargetRef
                        targetLabel: root.annotationTargetRef
                            ? (root.annotationTargetRef.kind === "starMapNode" ? qsTr("星图节点")
                               : root.annotationTargetRef.kind === "starMapEmbed" ? qsTr("子星图") : root.starmapTitle)
                            : qsTr("星图对象")
                        onCloseRequested: root.closeRightPane()
                        onTargetActivated: function(target) { root.annotationTargetRequested(target) }
                        onLinkPickerRequested: function(sourceRef, link) {
                            root.pendingLinkSource = sourceRef
                            root.pendingLinkId = link ? String(link.linkId || "") : ""
                            root.pendingLinkTarget = link ? link.target : null
                            annotationLinkPicker.open()
                        }
                    }

                    StarMapToolRail {
                        id: starMapRail
                        Layout.preferredWidth: root.toolRailWidth
                        Layout.minimumWidth: root.toolRailWidth
                        Layout.maximumWidth: root.toolRailWidth
                        Layout.fillHeight: true
                        railWidth: root.toolRailWidth
                        dt: root.dt
                        selectedTool: root.selectedTool
                        drawerOpen: root.toolDrawerOpen
                        aiAvailable: root.aiAvailable
                        aiEnabled: root.aiEnabled
                        agentAvailable: root.agentCliBackendRef
                            && String(root.agentCliBackendRef.selected_agent_id || "") !== ""
                        onToolRequested: function(key) {
                            root.requestToolPane(key)
                        }
                        onDrawerToggled: root.toggleRightPane()
                    }
                }
            }
        }
    }

    // ── 左右栏贴边折叠把手（嵌入模式不出现） ──
    Rectangle {
        id: leftPaneHandle
        visible: !root.embedded
        z: 50
        width: Kirigami.Units.gridUnit
        height: Kirigami.Units.gridUnit * 3.5
        radius: width / 2
        color: leftPaneHandleHover.containsMouse || leftPaneHandleHover.pressed
                   ? Kirigami.Theme.alternateBackgroundColor
                   : Kirigami.Theme.backgroundColor
        border.color: leftPaneHandleHover.containsMouse ? Kirigami.Theme.focusColor
                                                        : Kirigami.Theme.disabledTextColor
        border.width: 1
        x: root.leftPaneCollapsed
           ? Kirigami.Units.smallSpacing
           : Math.max(0, Math.min(root.width - width - Kirigami.Units.smallSpacing,
                                  paneSplitView.x + leftPaneContainer.x
                                  + leftPaneContainer.width - width / 2))
        y: Math.round((root.height - height) / 2)

        Behavior on color { ColorAnimation { duration: Kirigami.Units.shortDuration } }
        Behavior on border.color { ColorAnimation { duration: Kirigami.Units.shortDuration } }

        Kirigami.Icon {
            anchors.centerIn: parent
            source: root.leftPaneCollapsed ? "arrow-right" : "arrow-left"
            color: Kirigami.Theme.textColor
            implicitWidth: Kirigami.Units.iconSizes.smallMedium
            implicitHeight: Kirigami.Units.iconSizes.smallMedium
        }

        MouseArea {
            id: leftPaneHandleHover
            anchors.fill: parent
            anchors.margins: -Kirigami.Units.smallSpacing
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: root.toggleLeftPane()
        }
    }

    Rectangle {
        id: rightPaneHandle
        visible: !root.embedded
        z: 50
        width: Kirigami.Units.gridUnit
        height: Kirigami.Units.gridUnit * 3.5
        radius: width / 2
        color: rightPaneHandleHover.containsMouse || rightPaneHandleHover.pressed
                   ? Kirigami.Theme.alternateBackgroundColor
                   : Kirigami.Theme.backgroundColor
        border.color: rightPaneHandleHover.containsMouse ? Kirigami.Theme.focusColor
                                                         : Kirigami.Theme.disabledTextColor
        border.width: 1
        x: root.rightPaneCollapsed
           ? root.width - width - Kirigami.Units.smallSpacing
           : Math.max(0, Math.min(root.width - width - Kirigami.Units.smallSpacing,
                                  paneSplitView.x + rightPaneContainer.x - width / 2))
        y: Math.round((root.height - height) / 2)

        Behavior on color { ColorAnimation { duration: Kirigami.Units.shortDuration } }
        Behavior on border.color { ColorAnimation { duration: Kirigami.Units.shortDuration } }

        Kirigami.Icon {
            anchors.centerIn: parent
            source: root.rightPaneCollapsed ? "arrow-left" : "arrow-right"
            color: Kirigami.Theme.textColor
            implicitWidth: Kirigami.Units.iconSizes.smallMedium
            implicitHeight: Kirigami.Units.iconSizes.smallMedium
        }

        MouseArea {
            id: rightPaneHandleHover
            anchors.fill: parent
            anchors.margins: -Kirigami.Units.smallSpacing
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: root.toggleRightPane()
        }
    }

    Popup {
        id: annotationLinkPicker
        modal: true
        anchors.centerIn: Overlay.overlay
        width: Math.min(620, Overlay.overlay ? Overlay.overlay.width - 48 : 620)
        height: Math.min(560, Overlay.overlay ? Overlay.overlay.height - 48 : 560)
        padding: 0
        contentItem: AnnotationLinkPicker {
            dt: root.dt
            searchBackendRef: root.searchBackendRef
            projectId: root.annotationProjectId
            sourceRef: root.pendingLinkSource
            initialTargets: root.pendingLinkTarget ? [root.pendingLinkTarget] : []
            onCancelled: annotationLinkPicker.close()
            onTargetsSelected: function(targets) {
                if (!root.annotationBackendRef || !root.pendingLinkSource) return
                for (var i = 0; i < targets.length; ++i) {
                    if (i === 0 && root.pendingLinkId) {
                        root.annotationBackendRef.update_link_json(
                            String(root.pendingLinkSource.projectId), root.pendingLinkId,
                            JSON.stringify(root.pendingLinkSource), JSON.stringify(targets[i]))
                    } else {
                        root.annotationBackendRef.create_link_json(
                            JSON.stringify(root.pendingLinkSource), JSON.stringify(targets[i]))
                    }
                }
                root.pendingLinkId = ""
                root.pendingLinkTarget = null
                annotationLinkPicker.close()
                Qt.callLater(starMapTools.refresh)
            }
        }
    }

    Dialog {
        id: registerNameDialog
        title: qsTr("登记名称")
        modal: true
        anchors.centerIn: Overlay.overlay
        standardButtons: Dialog.Ok | Dialog.Cancel
        contentItem: TextField { id: nameField; text: root.pendingName }
        onAccepted: {
            if (!root.annotationBackendRef || !root.annotationProjectId || !nameField.text.trim()) return
            var raw = root.annotationBackendRef.create_named_entry_json(
                root.annotationProjectId, nameField.text.trim(), "[]", "[]", true)
            var envelope = null
            try { envelope = JSON.parse(String(raw || "{}")) } catch (e) {}
            var entry = envelope && envelope.success === true ? envelope.data : null
            if (entry && entry.entryId && root.pendingLinkSource) {
                var namedTarget = { objectRef: {
                    kind: "namedEntry", projectId: root.annotationProjectId,
                    objectId: String(entry.entryId)
                } }
                root.annotationBackendRef.create_link_json(
                    JSON.stringify(root.pendingLinkSource), JSON.stringify(namedTarget))
                Qt.callLater(starMapTools.refresh)
            }
        }
    }

    Dialog {
        id: treeRenameDialog
        title: qsTr("重命名")
        modal: true
        anchors.centerIn: Overlay.overlay
        standardButtons: Dialog.Ok | Dialog.Cancel
        contentItem: TextField {
            id: treeRenameField
            placeholderText: qsTr("名称")
            onAccepted: treeRenameDialog.accept()
        }
        onOpened: {
            treeRenameField.forceActiveFocus()
            treeRenameField.selectAll()
        }
        onAccepted: {
            root.renameTreeRow(root.pendingTreeRow, treeRenameField.text)
            root.pendingTreeRow = null
        }
    }

    Dialog {
        id: treeDeleteDialog
        title: qsTr("删除")
        modal: true
        anchors.centerIn: Overlay.overlay
        standardButtons: Dialog.Ok | Dialog.Cancel
        contentItem: Label {
            text: root.pendingTreeRow
                ? qsTr("确定删除“%1”？").arg(String(root.pendingTreeRow.title || ""))
                : ""
            wrapMode: Text.Wrap
        }
        onAccepted: {
            root.deleteTreeRow(root.pendingTreeRow)
            root.pendingTreeRow = null
        }
    }
}
