// =============================================================================
// SettingsRow.qml — 设置行组件
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 基础组件）
// 职责：单个设置项的行布局（标题 + 描述 + 内容区域）
// 约束：
//   - 纯 UI 组件，内容通过 default property 传入
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Layouts

Rectangle {
    id: root
    property var dt: null
    DesignTokens { id: fallbackDt }
    readonly property var resolvedDt: dt || fallbackDt
    property string title: ""
    property string description: ""
    property bool clickable: false
    signal clicked()
    default property alias controlData: controlHost.data

    color: rowHover.containsMouse && root.clickable ? resolvedDt.surfaceContainer : "transparent"
    radius: resolvedDt.radiusMd

    readonly property int _verticalPadding: resolvedDt.sp12
    readonly property int _controlMinHeight: resolvedDt.settingsControlHeight
    implicitHeight: Math.max(_controlMinHeight, textCol.implicitHeight) + _verticalPadding * 2
    implicitWidth: row.implicitWidth + resolvedDt.sp16

    RowLayout {
        id: row
        anchors.fill: parent
        anchors.leftMargin: resolvedDt.sp8
        anchors.rightMargin: resolvedDt.sp8
        anchors.topMargin: root._verticalPadding
        anchors.bottomMargin: root._verticalPadding
        spacing: resolvedDt.sp12

        ColumnLayout {
            id: textCol
            Layout.fillWidth: true
            Layout.alignment: Qt.AlignVCenter
            spacing: 2
            AppText {
                dt: root.resolvedDt
                text: root.title
                color: resolvedDt.textPrimary
                font.pixelSize: resolvedDt.body
                font.family: resolvedDt.fontFamily
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
            AppText {
                dt: root.resolvedDt
                text: root.description
                color: resolvedDt.textSecondary
                font.pixelSize: resolvedDt.caption
                font.family: resolvedDt.fontFamily
                visible: text.length > 0
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
        }

        Item {
            id: controlHost
            Layout.alignment: Qt.AlignRight | Qt.AlignVCenter
            Layout.preferredWidth: children.length > 0 ? Math.max(1, children[0].implicitWidth || children[0].width || 0) : 0
            implicitWidth: Layout.preferredWidth
            Layout.preferredHeight: children.length > 0 ? Math.max(root._controlMinHeight, children[0].implicitHeight || children[0].height || 0) : root._controlMinHeight
            implicitHeight: Layout.preferredHeight

            readonly property Item controlItem: children.length > 0 ? children[0] : null

            Binding {
                target: controlHost.controlItem
                property: "width"
                value: controlHost.width
                when: controlHost.controlItem !== null
            }

            Binding {
                target: controlHost.controlItem
                property: "height"
                value: controlHost.height
                when: controlHost.controlItem !== null
            }
        }
    }

    Rectangle {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        height: 1
        color: resolvedDt.border
        opacity: 0.65
    }

    MouseArea {
        id: rowHover
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        anchors.left: parent.left
        width: controlHost.children.length > 0
            ? Math.max(0, controlHost.mapToItem(root, 0, 0).x - row.spacing)
            : parent.width
        enabled: root.clickable
        hoverEnabled: true
        cursorShape: enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
        onClicked: root.clicked()
    }
}
