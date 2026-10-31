// =============================================================================
// StarMapNavigationTree.qml — 星图层级导航树
// =============================================================================
//
// 层级：Linux_qt UI 层
// 职责：星图工作区左侧的唯一层级导航。使用 Qt 6.8 原生 TreeView +
//   TreeViewDelegate，数据来自 Rust 的 StarmapTreeModel（QAbstractItemModel）。
//
// 身份：树行不是“文件系统目录”，而是 rootStarmapId + 完整 route segments。
//   可导航行用 routeKey；普通节点没有可进入画布，用 objectKey 区分并定位。
//   同一个子星图通过不同 Embed / Portal 到达时拥有不同 routeKey，必须分别选中。
//
// 交互：
//   - 单击可导航行：Workspace 让 Canvas 按该行的完整路由切换活动画布；
//   - 单击普通节点：Workspace 让 Canvas 在当前画布命中、选中并居中该对象；
//   - 展开箭头：只有 Embed / Portal 行可展开，第一次展开时才加载下一层；
//   - 右键行：重命名 / 删除，交给 Workspace 复用 starmapBackend 的方法；
//   - 顶部 +：在当前画布新建节点或子星图，不再创建独立根星图。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQml.Models
import SujianApp 1.0
import org.kde.kirigami as Kirigami

Rectangle {
    id: root

    required property var dt
    property var starmapBackendRef: null

    // 当前工作区唯一的根星图；切换根时整棵树重载，不继承旧根的展开/选中。
    property string workspaceRootStarmapId: ""

    // Canvas 当前路由；由 Workspace 在进入 / 退出子星图时同步。
    property string currentRootStarmapId: ""
    property string currentSegmentsJson: "[]"
    property string currentRouteKey: ""
    // 当前选中的普通节点对象键；为空时按 routeKey 高亮可导航行。
    property string currentObjectKey: ""
    property var contextRow: null
    // 已经成功建出根行的根 ID；用于区分“首次/换根”与同一根普通刷新。
    property string loadedRootStarmapId: ""

    signal routeActivated(string rootStarmapId, string segmentsJson, string routeKey)
    signal objectActivated(string rootStarmapId, string segmentsJson,
                           string kind, string objectId, string objectKey)
    signal createObjectRequested(string mode)
    signal renameRequested(var rowInfo)
    signal deleteRequested(var rowInfo)

    color: Kirigami.Theme.alternateBackgroundColor

    StarmapTreeModel {
        id: treeModel
    }

    function rowInfoOf(model) {
        if (!model)
            return null
        return {
            title: String(model.title || ""),
            starmapId: String(model.starmapId || ""),
            rootStarmapId: String(model.rootStarmapId || ""),
            routeSegments: String(model.routeSegments || "[]"),
            routeKey: String(model.routeKey || ""),
            objectKey: String(model.objectKey || ""),
            objectId: String(model.objectId || ""),
            rowKind: String(model.rowKind || "root"),
            parentStarmapId: String(model.parentStarmapId || ""),
            instanceId: String(model.instanceId || ""),
            nodeId: String(model.nodeId || ""),
            revision: Number(model.revision || 0),
            depth: Number(model.depth || 0)
        }
    }

    function expandedRouteKeys() {
        var keys = []
        if (!treeModel)
            return keys
        try {
            var parsed = JSON.parse(String(treeModel.expanded_keys_json() || "[]"))
            if (Array.isArray(parsed))
                keys = parsed.map(function(key) { return String(key) })
        } catch (e) {
            keys = []
        }
        return keys
    }

    // 当前 route 的根及每一级祖先 routeKey，用于把 expandToIndex() 在
    // TreeView 里展开的路径同步回模型，刷新时才能恢复。
    function clearExpandedDescendants(routeKey) {
        if (routeKey === "")
            return
        var keys = expandedRouteKeys()
        var prefix = routeKey.slice(-1) === "|" ? routeKey : routeKey + "/"
        for (var i = 0; i < keys.length; ++i) {
            var key = keys[i]
            if (key !== routeKey && key.indexOf(prefix) === 0)
                treeModel.set_expanded(key, false)
        }
    }

    function routeKeysForPath(rootStarmapId, segmentsJson) {
        var keys = []
        if (!rootStarmapId)
            return keys
        keys.push(String(rootStarmapId) + "|")
        var segments = []
        try {
            segments = JSON.parse(String(segmentsJson || "[]"))
        } catch (e) {
            return keys
        }
        if (!Array.isArray(segments))
            return keys
        var parts = []
        for (var i = 0; i < segments.length; ++i) {
            var segment = segments[i]
            if (!segment)
                continue
            if (segment.type === "enterPortal")
                parts.push("enterPortal:" + String(segment.nodeId || ""))
            else
                parts.push("enterEmbed:" + String(segment.instanceId || ""))
            keys.push(String(rootStarmapId) + "|" + parts.join("/"))
        }
        return keys
    }

    function reload(forceRootExpanded) {
        if (!treeModel)
            return
        if (forceRootExpanded === undefined)
            forceRootExpanded = root.workspaceRootStarmapId !== ""
                && (root.workspaceRootStarmapId !== root.loadedRootStarmapId
                    || root.currentRouteKey === "")
        // reload 前保存已加载分支的完整 routeKey 和当前普通节点对象键；
        // 刷新后只恢复当前根下仍存在的路径，不把旧根的展开/选中带过来。
        // 根切换时强制展开新根；同一根普通刷新时保持用户原有展开/折叠。
        var expandedKeys = forceRootExpanded ? [] : expandedRouteKeys()
        var selectedObjectKey = root.currentObjectKey
        treeModel.set_backend_ref(root.starmapBackendRef)
        treeModel.set_root_starmap_id(root.workspaceRootStarmapId)
        treeModel.refresh()
        treeView.forceLayout()

        // Qt 6.8 的 expandToIndex() 在根行已经可见时会直接返回，不能保证
        // 根行的子行展开。这里显式取根行并调用 expand(rootRow)。
        var rootKey = String(root.workspaceRootStarmapId || "") + "|"
        if (root.workspaceRootStarmapId !== "") {
            treeModel.ensure_children_loaded(rootKey)
            treeView.forceLayout()
        }
        var rootIndex = treeModel.index_for_key(rootKey)
        var rootRow = rootIndex ? treeView.rowAtIndex(rootIndex) : -1
        if (rootRow >= 0)
            root.loadedRootStarmapId = String(root.workspaceRootStarmapId || "")
        if (rootRow >= 0 && (forceRootExpanded || expandedKeys.indexOf(rootKey) >= 0)) {
            treeView.expand(rootRow)
            if (treeView.isExpanded(rootRow))
                treeModel.set_expanded(rootKey, true)
            treeView.forceLayout()
        }

        for (var i = 0; i < expandedKeys.length; ++i) {
            var key = expandedKeys[i]
            if (key === rootKey)
                continue
            var index = treeModel.index_for_key(key)
            if (!index)
                continue
            var row = treeView.rowAtIndex(index)
            if (row < 0)
                continue
            treeView.expand(row)
            if (treeView.isExpanded(row))
                treeModel.set_expanded(key, true)
            treeView.forceLayout()
        }
        if (selectedObjectKey !== "") {
            var objectIndex = treeModel.index_for_object_key(selectedObjectKey)
            if (objectIndex) {
                var objectRow = treeView.rowAtIndex(objectIndex)
                if (objectRow >= 0) {
                    if (treeView.selectionModel)
                        treeView.selectionModel.setCurrentIndex(
                            objectIndex, ItemSelectionModel.ClearAndSelect)
                    treeView.positionViewAtRow(objectRow, Qt.AlignVCenter)
                }
            }
        }
        if (root.currentRouteKey !== "")
            syncRoute(root.currentRootStarmapId, root.currentSegmentsJson,
                      root.currentRouteKey, false)
    }

    function syncRoute(rootStarmapId, segmentsJson, routeKey, expandPath) {
        root.currentRootStarmapId = String(rootStarmapId || "")
        root.currentSegmentsJson = String(segmentsJson || "[]")
        root.currentRouteKey = String(routeKey || "")
        if (!treeModel || root.currentRouteKey === "" || root.currentRootStarmapId === "")
            return
        if (expandPath === undefined)
            expandPath = true
        if (expandPath) {
            // 根行必须先显式展开：expandToIndex() 不会处理已经可见的根行。
            var rootKey = root.currentRootStarmapId + "|"
            treeModel.ensure_children_loaded(rootKey)
            var rootIndex = treeModel.index_for_key(rootKey)
            var rootRow = rootIndex ? treeView.rowAtIndex(rootIndex) : -1
            if (rootRow >= 0) {
                treeView.expand(rootRow)
                if (treeView.isExpanded(rootRow))
                    treeModel.set_expanded(rootKey, true)
                treeView.forceLayout()
            }
            var targetIndex = treeModel.index_for_route(root.currentRootStarmapId,
                                                        root.currentSegmentsJson)
            if (String(root.currentSegmentsJson || "[]") !== "[]")
                treeView.expandToIndex(targetIndex)
            treeView.forceLayout()
            // expandToIndex() 只改变视图；把实际展开的祖先路径同步回模型。
            var pathKeys = routeKeysForPath(root.currentRootStarmapId,
                                            root.currentSegmentsJson)
            for (var k = 0; k < pathKeys.length; ++k) {
                var pathIndex = treeModel.index_for_key(pathKeys[k])
                var pathRow = treeView.rowAtIndex(pathIndex)
                if (pathRow >= 0 && treeView.isExpanded(pathRow))
                    treeModel.set_expanded(pathKeys[k], true)
            }
        }
        var index = treeModel.index_for_route(root.currentRootStarmapId, root.currentSegmentsJson)
        var row = treeView.rowAtIndex(index)
        if (row < 0)
            return
        if (treeView.selectionModel)
            treeView.selectionModel.setCurrentIndex(index, ItemSelectionModel.ClearAndSelect)
        treeView.positionViewAtRow(row, Qt.AlignVCenter)
    }

    function activateRow(model) {
        var info = rowInfoOf(model)
        if (!info)
            return
        if (info.rowKind === "node") {
            if (info.objectKey === "" || info.nodeId === "")
                return
            root.currentObjectKey = info.objectKey
            root.objectActivated(info.rootStarmapId, info.routeSegments,
                                 "node", info.nodeId, info.objectKey)
            return
        }
        if (info.routeKey === "")
            return
        root.currentObjectKey = ""
        root.currentRouteKey = info.routeKey
        root.routeActivated(info.rootStarmapId, info.routeSegments, info.routeKey)
    }

    function openRowMenu(model) {
        root.contextRow = rowInfoOf(model)
        if (!root.contextRow)
            return
        rowMenu.popup()
    }

    function ensureExpanded(row, model) {
        var key = model ? String(model.routeKey || "") : ""
        if (key === "" || row < 0 || String(model.rowKind || "") === "node")
            return
        // Issue #1016：箭头是真正的展开/收起切换，不再只 expand。
        if (treeView.isExpanded(row)) {
            treeView.collapse(row)
            treeModel.set_expanded(key, false)
            clearExpandedDescendants(key)
            return
        }
        if (treeModel.ensure_children_loaded(key)) {
            treeView.expand(row)
            treeModel.set_expanded(key, true)
        }
    }

    onStarmapBackendRefChanged: reload()

    onWorkspaceRootStarmapIdChanged: {
        if (String(root.workspaceRootStarmapId) !== String(root.currentRootStarmapId)) {
            root.currentRootStarmapId = String(root.workspaceRootStarmapId)
            root.currentSegmentsJson = "[]"
            root.currentRouteKey = ""
            root.currentObjectKey = ""
        }
        reload(true)
    }

    Component.onCompleted: reload(true)

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: Kirigami.Units.smallSpacing
            Layout.rightMargin: Kirigami.Units.smallSpacing
            Layout.topMargin: Kirigami.Units.smallSpacing
            spacing: Kirigami.Units.smallSpacing

            Label {
                Layout.fillWidth: true
                text: qsTr("星图")
                font.bold: true
                elide: Text.ElideRight
            }

            ToolButton {
                icon.name: "view-refresh"
                display: AbstractButton.IconOnly
                onClicked: root.reload()
                ToolTip.visible: hovered
                ToolTip.text: qsTr("刷新星图树")
            }

            ToolButton {
                icon.name: "list-add"
                display: AbstractButton.IconOnly
                onClicked: createMenu.popup()
                ToolTip.visible: hovered
                ToolTip.text: qsTr("在当前星图中新建")
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 1
            color: Kirigami.Theme.disabledTextColor
            opacity: 0.35
        }

        TreeView {
            id: treeView
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: treeModel
            // 行点击由 delegate 自己分派“切换画布”；展开只由箭头触发，
            // 避免点击行的同时误进入另一个星图。
            pointerNavigationEnabled: false

            delegate: TreeViewDelegate {
                id: delegate

                indentation: Kirigami.Units.gridUnit
                leftMargin: Kirigami.Units.smallSpacing
                rightMargin: Kirigami.Units.smallSpacing
                highlighted: root.currentObjectKey !== ""
                    ? String(delegate.model.objectKey || "") === root.currentObjectKey
                    : (root.currentRouteKey !== ""
                       && String(delegate.model.routeKey || "") === root.currentRouteKey)

                ToolTip.visible: hovered
                ToolTip.text: String(delegate.model.title || "")

                onClicked: root.activateRow(delegate.model)

                TapHandler {
                    acceptedButtons: Qt.RightButton
                    onSingleTapped: root.openRowMenu(delegate.model)
                }

                indicator: Item {
                    id: indicatorItem
                    readonly property bool hasAnyChildren: delegate.isTreeNode
                        && String(delegate.model.rowKind || "") !== "node"
                        && (delegate.hasChildren
                            || (delegate.model && delegate.model.hasChildren === true))
                    visible: hasAnyChildren
                    implicitWidth: visible ? Kirigami.Units.gridUnit : 0
                    implicitHeight: Kirigami.Units.gridUnit * 1.75
                    x: !delegate.mirrored
                       ? delegate.leftMargin + delegate.depth * delegate.indentation
                       : delegate.width - delegate.leftMargin
                         - delegate.depth * delegate.indentation - width

                    Kirigami.Icon {
                        anchors.centerIn: parent
                        source: delegate.expanded ? "arrow-down" : "arrow-right"
                        color: Kirigami.Theme.textColor
                        implicitWidth: Kirigami.Units.iconSizes.small
                        implicitHeight: Kirigami.Units.iconSizes.small
                    }

                    TapHandler {
                        onSingleTapped: root.ensureExpanded(delegate.row, delegate.model)
                    }
                }
            }

            ScrollBar.vertical: ScrollBar {}
        }

        Label {
            Layout.fillWidth: true
            Layout.margins: Kirigami.Units.smallSpacing
            visible: treeView.rows === 0
            text: qsTr("暂无星图")
            color: Kirigami.Theme.disabledTextColor
            horizontalAlignment: Text.AlignHCenter
        }
    }

    Menu {
        id: createMenu

        MenuItem {
            text: qsTr("新建节点")
            onTriggered: root.createObjectRequested("node")
        }

        MenuItem {
            text: qsTr("新建子星图")
            onTriggered: root.createObjectRequested("starmap")
        }
    }

    Menu {
        id: rowMenu

        MenuItem {
            text: qsTr("重命名")
            enabled: root.contextRow !== null
            onTriggered: {
                if (root.contextRow)
                    root.renameRequested(root.contextRow)
            }
        }

        MenuItem {
            text: qsTr("删除")
            enabled: root.contextRow !== null
            onTriggered: {
                if (root.contextRow)
                    root.deleteRequested(root.contextRow)
            }
        }
    }
}
