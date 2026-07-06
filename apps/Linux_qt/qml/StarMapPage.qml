// =============================================================================
// StarMapPage.qml — 星图列表页（施工中占位）
// =============================================================================
//
// 层级：Desktop UI 层（QML 页面）
// 职责：星图功能施工中占位页，仅展示标题、说明和返回入口
// 约束：
//   - 不暴露半成品编辑器、节点拖拽或不可用按钮
//   - 保留原有属性和信号，供其他 QML 文件引用
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    property var dt: null
    property var backendRef: null
    property var starMapController: null
    property var appState: ({})
    property var starmaps: []
    property string filterProjectId: ""

    signal openStarmap(string starmapId, string title)

    color: dt.bg

    Component.onCompleted: {}
    onVisibleChanged: {}

    HubPageFrame {
        id: pageFrame
        anchors.fill: parent
        dt: root.dt

        headerData: RowLayout {
            spacing: dt.sp12
            Layout.fillWidth: true
            Layout.fillHeight: true

            AppButton {
                dt: root.dt
                variant: "text"
                text: qsTr("← 返回")
                onClicked: root.visible = false
            }

            Item { Layout.fillWidth: true }

            AppText {
                dt: root.dt
                text: qsTr("星图")
                color: dt.onSurface
                font.pixelSize: dt.headline
                font.family: dt.fontFamily
                font.weight: Font.DemiBold
            }
        }

        contentData: ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: dt.sp16

            Item { Layout.fillHeight: true }

            AppText {
                dt: root.dt
                Layout.alignment: Qt.AlignHCenter
                text: qsTr("[ 施工中 ]")
                color: dt.onSurfaceVariant
                font.pixelSize: dt.headline
                font.family: dt.fontFamily
                font.weight: Font.DemiBold
            }

            AppText {
                dt: root.dt
                Layout.alignment: Qt.AlignHCenter
                text: qsTr("星图功能正在施工中，敬请期待")
                color: dt.onSurfaceVariant
                font.pixelSize: dt.body
                font.family: dt.fontFamily
            }

            Item { Layout.fillHeight: true }
        }
    }
}