// =============================================================================
// StarMapNavigationTree.qml — 星图层级导航树
// =============================================================================
//
// 层级：Linux_qt UI 层
// 职责：星图工作区左侧的唯一层级导航。Issue #1048 起不再使用
//   TreeView + TreeViewDelegate：Rust 的 StarmapTreeModel 仍作为唯一数据源，
//   通过 visible_rows_json() 输出按展开状态深度优先展开的扁平行快照，
//   本文件用 ListView + 手绘 StarMapTreeRow 渲染静态行。
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
//   - 目录行本身不可拖动；滚轮只滚动 ListView，不改变任何行的几何或排序。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
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
    // 模型按展开状态输出的扁平行快照。
    property var rows: []

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
            assetId: String(model.assetId || ""),
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

    function reloadRows() {
        if (!treeModel) {
            rows = []
            return
        }
        try {
            var parsed = JSON.parse(String(treeModel.visible_rows_json() || "[]"))
            rows = Array.isArray(parsed) ? parsed : []
        } catch (e) {
            rows = []
        }
    }

    function rowIndexForKey(routeKey) {
        for (var i = 0; i < rows.length; ++i)
            if (String(rows[i].routeKey || "") === String(routeKey))
                return i
        return -1
    }

    function rowIndexForObjectKey(objectKey) {
        for (var i = 0; i < rows.length; ++i)
            if (String(rows[i].objectKey || "") === String(objectKey))
                return i
        return -1
    }

    // 当前 route 的根及每一级祖先 routeKey。
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

    function expandModelPath(rootStarmapId, segmentsJson) {
        var keys = routeKeysForPath(rootStarmapId, segmentsJson)
        if (keys.length === 0)
            return
        // 目标是根行时展开根；否则展开目标的全部祖先，目标本身保持原状。
        var lastAncestor = keys.length === 1 ? 0 : keys.length - 2
        for (var i = 0; i <= lastAncestor; ++i) {
            treeModel.ensure_children_loaded(keys[i])
            treeModel.set_expanded(keys[i], true)
        }
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
        var expandedKeys = forceRootExpanded ? [] : expandedRouteKeys()
        var selectedObjectKey = root.currentObjectKey
        treeModel.set_backend_ref(root.starmapBackendRef)
        treeModel.set_root_starmap_id(root.workspaceRootStarmapId)
        treeModel.refresh()

        var rootKey = String(root.workspaceRootStarmapId || "") + "|"
        if (root.workspaceRootStarmapId !== "") {
            treeModel.ensure_children_loaded(rootKey)
            root.loadedRootStarmapId = String(root.workspaceRootStarmapId || "")
            if (forceRootExpanded || expandedKeys.indexOf(rootKey) >= 0)
                treeModel.set_expanded(rootKey, true)
        }

        for (var i = 0; i < expandedKeys.length; ++i) {
            var key = expandedKeys[i]
            if (key === rootKey)
                continue
            treeModel.ensure_children_loaded(key)
            treeModel.set_expanded(key, true)
        }
        reloadRows()

        if (root.currentRouteKey !== "")
            syncRoute(root.currentRootStarmapId, root.currentSegmentsJson,
                      root.currentRouteKey, false)
        if (selectedObjectKey !== "") {
            var objectIndex = rowIndexForObjectKey(selectedObjectKey)
            if (objectIndex >= 0)
                treeList.positionViewAtIndex(objectIndex, ListView.Center)
        }
    }

    function syncRoute(rootStarmapId, segmentsJson, routeKey, expandPath) {
        root.currentRootStarmapId = String(rootStarmapId || "")
        root.currentSegmentsJson = String(segmentsJson || "[]")
        root.currentRouteKey = String(routeKey || "")
        if (!treeModel || root.currentRouteKey === "" || root.currentRootStarmapId === "")
            return
        if (expandPath === undefined)
            expandPath = true
        if (expandPath)
            expandModelPath(root.currentRootStarmapId, root.currentSegmentsJson)
        reloadRows()
        var row = rowIndexForKey(root.currentRouteKey)
        if (row >= 0)
            treeList.positionViewAtIndex(row, ListView.Center)
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

    function ensureExpanded(model) {
        var key = model ? String(model.routeKey || "") : ""
        if (key === "" || String(model.rowKind || "") === "node")
            return
        if (model.expanded === true) {
            // Issue #1016：收起父行要一并收起后代，避免下次展开时把旧子树
            // 整段弹回来。当前 rows 快照仍包含这些后代，先收集再逐行复位。
            collapseDescendants(key)
            treeModel.set_expanded(key, false)
        } else if (treeModel.ensure_children_loaded(key)) {
            treeModel.set_expanded(key, true)
        }
        reloadRows()
    }

    function collapseDescendants(routeKey) {
        if (routeKey === "")
            return
        var prefix = routeKey.slice(-1) === "|" ? routeKey : routeKey + "/"
        var descendants = []
        for (var i = 0; i < rows.length; ++i) {
            var key = String(rows[i].routeKey || "")
            if (key !== routeKey && key.indexOf(prefix) === 0
                    && rows[i].expanded === true)
                descendants.push(key)
        }
        for (var j = 0; j < descendants.length; ++j)
            treeModel.set_expanded(descendants[j], false)
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

        ListView {
            id: treeList
            // Issue #1048 复审：Qt 6.9+ Flickable 默认接受左键拖动，会移动整个
            // 列表内容。目录行改为手绘后，左键只属于行的点击/展开；滚轮、
            // 触控板、滚动条与触屏滚动仍由原生处理。
            acceptedButtons: Qt.NoButton
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: root.rows
            spacing: 0

            delegate: StarMapTreeRow {
                width: treeList.width
                selected: root.currentObjectKey !== ""
                    ? String(modelData.objectKey || "") === root.currentObjectKey
                    : (root.currentRouteKey !== ""
                       && String(modelData.routeKey || "") === root.currentRouteKey)
                onActivated: root.activateRow(rowData)
                onMenuRequested: root.openRowMenu(rowData)
                onExpansionToggled: root.ensureExpanded(rowData)
            }

            ScrollBar.vertical: ScrollBar {}
        }

        Label {
            Layout.fillWidth: true
            Layout.margins: Kirigami.Units.smallSpacing
            visible: root.rows.length === 0
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
