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
//   工作台不再持有任何子 Scene / 子 Content 引用，也不把焦点栈当画布身份。
//
// Issue #822：节点标题编辑已经移进节点自身的内联编辑，删掉 inspector 链路。
//
// Issue #977：删除本页自绘的 56px 顶栏和四边 border；返回 / 标题 / 公共入口
//   统一由 main.qml 的 DesktopTopBar 负责。Workspace 保留导航语义，暴露
//   focusIsRoot 与 navigateBack() 给壳层调用。
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
    property bool embedded: false
    property string selectedTool: "tags"
    property bool toolDrawerOpen: false
    property var pendingLinkSource: null
    property string pendingLinkId: ""
    property var pendingLinkTarget: null
    property string pendingName: ""

    // Issue #977：顶栏需要知道当前是根画布还是子画布，以决定返回文案与动作。
    readonly property bool focusIsRoot: canvas.focusIsRoot

    signal backClicked()
    signal requestSync()
    signal requestSearch()
    signal openSettings()
    signal sceneGeometryReady(string starmapId, string routeKey)
    signal annotationTargetRequested(var target)
    signal annotationToolRequested(string toolKey)

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

    // Issue #814 评论 5935285879: 当前活动画布共享的唯一选中状态控制器。
    // 由 Workspace 创建，传给 Canvas；切换活动画布时选中身份带新的 routeKey。
    StarMapSelectionController {
        id: sharedSelectionController
    }

    Rectangle {
        anchors.fill: parent
        color: Kirigami.Theme.backgroundColor

        RowLayout {
            anchors.fill: parent
            spacing: 0

            // 主体：唯一一块星图画布，工具抽屉只在独立星图工作区加入。
            StarMapCanvas {
                id: canvas
                Layout.fillWidth: true
                Layout.fillHeight: true
                dt: root.dt
                onSceneGeometryReady: function(starmapId, routeKey) {
                    root.sceneGeometryReady(starmapId, routeKey)
                }
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
                        root.toolDrawerOpen = true
                    }
                }

                onVisibleChanged: {
                    if (!visible)
                        canvas.resetInteraction()
                }
            }

            StarMapToolDrawer {
                id: starMapTools
                visible: !root.embedded && root.toolDrawerOpen
                Layout.preferredWidth: Math.min(340, Math.max(260, root.width * 0.32))
                Layout.minimumWidth: 240
                Layout.fillHeight: true
                dt: root.dt
                selectedTool: root.selectedTool
                isOpen: visible
                aiAvailable: root.aiAvailable
                aiEnabled: root.aiEnabled
                annotationBackendRef: root.annotationBackendRef
                searchBackendRef: root.searchBackendRef
                projectId: root.annotationProjectId
                targetRef: root.annotationTargetRef
                targetLabel: root.annotationTargetRef
                    ? (root.annotationTargetRef.kind === "starMapNode" ? qsTr("星图节点")
                       : root.annotationTargetRef.kind === "starMapEmbed" ? qsTr("子星图") : root.starmapTitle)
                    : qsTr("星图对象")
                onCloseRequested: root.toolDrawerOpen = false
                onTargetActivated: function(target) { root.annotationTargetRequested(target) }
                onLinkPickerRequested: function(sourceRef, link) {
                    root.pendingLinkSource = sourceRef
                    root.pendingLinkId = link ? String(link.linkId || "") : ""
                    root.pendingLinkTarget = link ? link.target : null
                    annotationLinkPicker.open()
                }
            }

            StarMapToolRail {
                visible: !root.embedded
                Layout.preferredWidth: 56
                Layout.minimumWidth: 56
                Layout.maximumWidth: 56
                Layout.fillHeight: true
                dt: root.dt
                selectedTool: root.selectedTool
                drawerOpen: root.toolDrawerOpen
                aiAvailable: root.aiAvailable
                aiEnabled: root.aiEnabled
                onToolRequested: function(key) {
                    root.selectedTool = key
                    root.toolDrawerOpen = true
                }
                onDrawerToggled: root.toolDrawerOpen = !root.toolDrawerOpen
            }
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
                }, externalUrl: null }
                root.annotationBackendRef.create_link_json(
                    JSON.stringify(root.pendingLinkSource), JSON.stringify(namedTarget))
                Qt.callLater(starMapTools.refresh)
            }
        }
    }
}
