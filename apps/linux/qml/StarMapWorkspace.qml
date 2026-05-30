// =============================================================================
// StarMapWorkspace.qml — 星图编辑工作区
// =============================================================================
//
// 层级：Linux UI 层（QML 页面）
// 职责：星图编辑器容器（画布 + 返回按钮）
// 约束：
//   - 纯布局容器，业务逻辑委托给 StarMapGraphController
//   - 包含 StarMapCanvas 进行可视化渲染
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: root
    width: parent ? parent.width : 800
    height: parent ? parent.height : 600

    property string starmapId: ""
    property string starmapTitle: qsTr("星图编辑器")
    property var dt: null
    property var backendRef: null

    signal backClicked()

    Rectangle {
        anchors.fill: parent
        color: dt ? dt.surface : "#1A1D23"
    }

    StarMapCanvas {
        id: canvas
        anchors.fill: parent
        starmapId: root.starmapId
        dt: root.dt
        backendRef: root.backendRef
    }

    // 浮动返回按钮
    AppButton {
        id: backBtn
        text: qsTr("← 返回")
        theme: root.dt
        variant: "secondary"
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.margins: dt ? dt.sp16 : 16
        z: 100
        onClicked: root.backClicked()
    }

    Component.onCompleted: {
        if (starmapId !== "") {
            canvas.loadGraph()
        }
    }
}
