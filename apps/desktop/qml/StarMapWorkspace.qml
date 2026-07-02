// =============================================================================
// StarMapWorkspace.qml — 星图施工占位页
// =============================================================================
//
// 层级：Desktop UI 层（QML 页面）
// 职责：星图功能施工占位，防止用户进入半成品编辑状态
// 约束：
//   - 纯占位页面，保留返回按钮
//   - 星图正式功能走 StarMapCapability 路线
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
    property var dt: null
    property var backendRef: null

    signal backClicked()

    Rectangle {
        anchors.fill: parent
        color: dt.surface

        ColumnLayout {
            anchors.centerIn: parent
            spacing: dt.sp16

            AppText {
                text: "\uD83C\uDF0C"
                dt: root.dt
                font.pixelSize: 48
                Layout.alignment: Qt.AlignHCenter
            }
            AppText {
                text: qsTr("星图正在施工")
                dt: root.dt
                color: dt.textPrimary
                font.pixelSize: dt.title
                font.weight: Font.Bold
                Layout.alignment: Qt.AlignHCenter
            }
            AppText {
                text: qsTr("星图功能将在后续版本实现，敬请期待")
                dt: root.dt
                color: dt.textSecondary
                font.pixelSize: dt.body
                Layout.alignment: Qt.AlignHCenter
            }
        }
    }

    AppButton {
        id: backBtn
        text: qsTr("← 返回")
        dt: root.dt
        variant: "secondary"
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.margins: dt.sp16
        z: 100
        onClicked: root.backClicked()
    }
}
