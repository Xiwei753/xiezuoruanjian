// =============================================================================
// EmptyWorkspace.qml — 空工作区引导页
// =============================================================================
//
// 层级：Desktop UI 层（QML UI 组件）
// 职责：工作区为空时的引导页面，提示用户创建工作区
// 约束：
//   - 纯展示组件，创建操作通过 signal 传递给 main.qml
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Qt.labs.platform as Platform

Item {
    id: root
    property var dt: null

    // Safe access: fallback to light-theme defaults when dt is null
    readonly property color _bg: dt ? dt.bg : "#FCFCFF"
    readonly property color _primaryContainer: dt ? dt.primaryContainer : "#CCE5FF"
    readonly property color _primary: dt ? dt.primary : "#006497"
    readonly property color _border: dt ? dt.border : "#71788057"
    readonly property color _textPrimary: dt ? dt.textPrimary : "#1A1C1E"
    readonly property color _textSecondary: dt ? dt.textSecondary : "#42474E"
    readonly property int _radiusXs: dt ? dt.radiusXs : 4
    readonly property int _sp8: dt ? dt.sp8 : 8
    readonly property int _sp12: dt ? dt.sp12 : 12
    readonly property int _sp24: dt ? dt.sp24 : 24
    readonly property int _title: dt ? dt.title : 24
    readonly property int _body: dt ? dt.body : 14
    readonly property string _fontFamily: dt ? dt.fontFamily : "sans-serif"

    property var backendRef: null

    signal createWorkspaceWithPath(string path)
    signal openWorkspaceWithPath(string path)
    signal initFromGithub()


    Platform.FolderDialog {
        id: createWorkspaceDialog
        title: qsTr("选择或创建空文件夹作为新工作区")
        onAccepted: {
            var path = String(folder).replace(/^(file:\/{2,3})|(qrc:\/{2})|(http:\/{2})/, "");
            if (/^\/[A-Za-z]:/.test(path)) path = path.substring(1);
            path = decodeURIComponent(path);
            root.createWorkspaceWithPath(path);
        }
    }

    Platform.FolderDialog {
        id: openWorkspaceDialog
        title: qsTr("选择已有工作区文件夹")
        onAccepted: {
            var path = String(folder).replace(/^(file:\/{2,3})|(qrc:\/{2})|(http:\/{2})/, "");
            if (/^\/[A-Za-z]:/.test(path)) path = path.substring(1);
            path = decodeURIComponent(path);
            root.openWorkspaceWithPath(path);
        }
    }

    Rectangle {
        anchors.fill: parent
        color: _bg
    }

    ColumnLayout {
        spacing: _sp24
        width: Math.min(parent.width - 80, 480)
        height: implicitHeight
        x: Math.max(0, Math.floor((parent.width - width) / 2))
        y: Math.max(0, Math.floor((parent.height - height) / 2))

        // Icon
        Rectangle {
            Layout.alignment: Qt.AlignHCenter
            width: 64; height: 64
            radius: 32
            color: _primaryContainer
            border.color: _border
            border.width: 1

            Rectangle {
                anchors.centerIn: parent
                width: 24; height: 24
                radius: _radiusXs
                color: "transparent"
                border.color: _primary
                border.width: 2
            }
        }

        // Title
        Text {
            Layout.alignment: Qt.AlignHCenter
            text: qsTr("未打开工作区")
            font.pixelSize: _title
            font.family: _fontFamily
            font.bold: true
            color: _textPrimary
        }

        // Description
        Text {
            Layout.alignment: Qt.AlignHCenter
            text: qsTr("选择或创建工作区后开始写作")
            font.pixelSize: _body
            font.family: _fontFamily
            color: _textSecondary
            horizontalAlignment: Text.AlignHCenter
            Layout.fillWidth: true
            wrapMode: Text.Wrap
        }

        // Actions
        ColumnLayout {
            Layout.alignment: Qt.AlignHCenter
            Layout.topMargin: _sp8
            spacing: _sp12
            width: 280

            AppButton {
                Layout.fillWidth: true
                text: qsTr("新建工作区")
                dt: root.dt
                variant: "primary"
                onClicked: createWorkspaceDialog.open()
            }

            AppButton {
                Layout.fillWidth: true
                text: qsTr("打开工作区")
                dt: root.dt
                variant: "secondary"
                onClicked: openWorkspaceDialog.open()
            }


        }
    }
}
