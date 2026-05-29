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
    property string starmapTitle: "星图编辑器"
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
    Button {
        id: backBtn
        text: "← 返回"
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.margins: 16
        z: 100

        contentItem: Text {
            text: backBtn.text
            font.pixelSize: 14
            font.bold: true
            color: backBtn.hovered ? (dt ? dt.accent : "#7B8CDE") : (dt ? dt.textPrimary : "#E2E4E9")
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
            Behavior on color { ColorAnimation { duration: 150 } }
        }

        background: Rectangle {
            implicitWidth: 80
            implicitHeight: 36
            color: backBtn.hovered ? (dt ? dt.card : "#2A2E36") : (dt ? dt.surface : "#1A1D23")
            border.color: backBtn.hovered ? (dt ? dt.accent : "#7B8CDE") : (dt ? dt.border : "#2A2E36")
            border.width: 1.5
            radius: 8
            Behavior on color { ColorAnimation { duration: 150 } }
            Behavior on border.color { ColorAnimation { duration: 150 } }
        }
    }

    Component.onCompleted: {
        if (starmapId !== "") {
            canvas.loadGraph()
        }
        backBtn.clicked.connect(root.backClicked)
    }
}
