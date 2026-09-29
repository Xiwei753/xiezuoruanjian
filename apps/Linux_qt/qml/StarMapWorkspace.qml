// =============================================================================
// StarMapWorkspace.qml — 星图工作区
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 页面）
// 职责：星图编辑工作区，包含工具栏、画布和检查器
// 约束：
//   - 纯 UI 层，业务逻辑通过 StarMapCanvas 和 StarMapInspector 委托
//   - 保留原有属性和信号，供 main.qml 引用
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: root
    width: parent ? parent.width : 800
    height: parent ? parent.height : 600

    property string starmapId: ""
    property string starmapTitle: qsTr("星图")
    required property var dt
    property var starmapBackendRef: null
    // Issue #790 评论 5875963057: 顶栏收口后的同步/搜索/设置入口
    property var appState: ({})

    signal backClicked()
    signal enterStarmapRequested(string starmapId, string title)
    signal requestSync()
    signal requestSearch()
    signal openSettings()

    Rectangle {
        anchors.fill: parent
        color: dt.bg

        ColumnLayout {
            anchors.fill: parent
            spacing: 0

            // 顶部工具栏：返回 + 标题 + 星图操作
            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 56
                color: dt.surface
                border.color: dt.border
                border.width: 1

                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: dt.sp16
                    anchors.rightMargin: dt.sp16
                    spacing: dt.sp12

                    AppButton {
                        dt: root.dt
                        variant: "text"
                        text: qsTr("← 返回")
                        onClicked: root.backClicked()
                    }

                    AppText {
                        dt: root.dt
                        text: root.starmapTitle
                        color: dt.onSurface
                        font.pointSize: dt.fontLgPt
                        font.family: dt.fontFamily
                        font.weight: Font.DemiBold
                        Layout.alignment: Qt.AlignVCenter
                    }

                    Item { Layout.fillWidth: true }

                    // Issue #790 评论 5875963057: 右侧公共入口收口到 GlobalTopActions
                    GlobalTopActions {
                        dt: root.dt
                        appState: root.appState
                        onRequestSync: root.requestSync()
                        onRequestSearch: root.requestSearch()
                        onOpenSettings: root.openSettings()
                    }
                }
            }

            // 主体：画布占满（Inspector 改为按需编辑浮层，Issue #791 评论 5883783849）
            RowLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: 0

                StarMapCanvas {
                    id: canvas
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    dt: root.dt
                    starmapId: root.starmapId
                    starmapBackendRef: root.starmapBackendRef

                    onEnterStarmapRequested: function(smId, smTitle) {
                        root.enterStarmapRequested(smId, smTitle)
                    }
                    onEditNodeRequested: function(node) {
                        inspectorPopup.selectedNode = node
                        inspectorPopup.open()
                    }
                }
            }
        }

        // Issue #791 评论 5883783849: 按需编辑浮层，不常驻不挤画布
        Popup {
            id: inspectorPopup
            modal: true
            focus: true
            width: 360
            height: 360
            anchors.centerIn: parent

            property var selectedNode: null

            contentItem: StarMapInspector {
                dt: root.dt
                starmapId: root.starmapId
                selectedNode: inspectorPopup.selectedNode
                selectedEdge: null

                onNodeUpdated: function(nodeId, patch) {
                    canvas.updateNodeFromInspector(nodeId, patch)
                }
                onNodeDeleted: function(nodeId) {
                    canvas.deleteNodeFromInspector(nodeId)
                    inspectorPopup.close()
                }
            }

            onClosed: {
                selectedNode = null
            }
        }
    }

    // 星图切换时先清瞬时交互状态再重新加载。
    // Issue #798: 返回父图 / 进入子图复用同一个 Workspace 实例，
    // 必须经过 resetInteraction 清掉旧 move/connect 状态，否则新图会继承旧 pointerMode。
    onStarmapIdChanged: {
        canvas.resetInteraction()
        if (starmapId.length > 0) canvas.loadGraph()
    }

    Component.onCompleted: {
        if (starmapId.length > 0) canvas.loadGraph()
    }
}
