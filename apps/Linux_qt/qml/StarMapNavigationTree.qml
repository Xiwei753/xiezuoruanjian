// =============================================================================
// StarMapNavigationTree.qml — 星图层级导航树
// =============================================================================
//
// 层级：Linux_qt UI 层
// 职责：星图工作区左侧的唯一层级导航。使用 Qt 6.8 原生 TreeView +
//   TreeViewDelegate，数据来自 Rust 的 StarmapTreeModel（QAbstractItemModel）。
//
// 身份：树行不是“文件系统目录”，而是 rootStarmapId + 完整 route segments。
//   同一个子星图通过不同 Embed / Portal 到达时拥有不同 routeKey，必须分别选中。
//
// 交互：
//   - 单击行：Workspace 让 Canvas 按该行的完整路由切换活动画布；
//   - 展开箭头：第一次展开时才让 Rust 模型调用 get_starmap_graph_json() 加载
//     子层（根层来自 list_root_starmaps_json()）；
//   - 右键行：重命名 / 删除，交给 Workspace 复用 starmapBackend 的方法；
//   - 顶部 +：新建根星图。
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

    // Canvas 当前路由；由 Workspace 在进入 / 退出子星图时同步。
    property string currentRootStarmapId: ""
    property string currentSegmentsJson: "[]"
    property string currentRouteKey: ""
    property var contextRow: null

    signal routeActivated(string rootStarmapId, string segmentsJson, string routeKey)
    signal createStarmapRequested()
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
            rowKind: String(model.rowKind || "root"),
            parentStarmapId: String(model.parentStarmapId || ""),
            instanceId: String(model.instanceId || ""),
            nodeId: String(model.nodeId || ""),
            revision: Number(model.revision || 0),
            depth: Number(model.depth || 0)
        }
    }

    function reload() {
        if (!treeModel)
            return
        treeModel.set_backend_ref(root.starmapBackendRef)
        treeModel.refresh()
        if (root.currentRouteKey !== "")
            syncRoute(root.currentRootStarmapId, root.currentSegmentsJson, root.currentRouteKey)
    }

    function syncRoute(rootStarmapId, segmentsJson, routeKey) {
        root.currentRootStarmapId = String(rootStarmapId || "")
        root.currentSegmentsJson = String(segmentsJson || "[]")
        root.currentRouteKey = String(routeKey || "")
        if (!treeModel || root.currentRouteKey === "" || root.currentRootStarmapId === "")
            return
        var index = treeModel.index_for_route(root.currentRootStarmapId, root.currentSegmentsJson)
        var row = treeView.rowAtIndex(index)
        if (row < 0)
            return
        treeView.expandToIndex(index)
        treeView.forceLayout()
        row = treeView.rowAtIndex(index)
        if (row < 0)
            return
        if (treeView.selectionModel)
            treeView.selectionModel.setCurrentIndex(index, ItemSelectionModel.ClearAndSelect)
        treeView.positionViewAtRow(row, Qt.AlignVCenter)
    }

    function activateRow(model) {
        var info = rowInfoOf(model)
        if (!info || info.routeKey === "")
            return
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
        if (key === "")
            return
        if (treeModel.ensure_children_loaded(key)) {
            if (!treeView.isExpanded(row))
                treeView.expand(row)
        }
    }

    onStarmapBackendRefChanged: reload()

    Component.onCompleted: reload()

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
                onClicked: root.createStarmapRequested()
                ToolTip.visible: hovered
                ToolTip.text: qsTr("新建星图")
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
                highlighted: root.currentRouteKey !== ""
                    && String(delegate.model.routeKey || "") === root.currentRouteKey

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
