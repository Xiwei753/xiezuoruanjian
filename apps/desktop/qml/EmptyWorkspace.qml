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
    property var appTheme: null
    property var backendRef: null

    signal createWorkspaceWithPath(string path)
    signal openWorkspaceWithPath(string path)


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
        color: root.appTheme ? root.appTheme.bg : "#f5f5f5"
    }

    ColumnLayout {
        spacing: root.appTheme ? root.appTheme.sp24 : 24
        width: Math.min(parent.width - 80, 480)
        height: implicitHeight
        x: Math.max(0, Math.floor((parent.width - width) / 2))
        y: Math.max(0, Math.floor((parent.height - height) / 2))

        // Icon
        Rectangle {
            Layout.alignment: Qt.AlignHCenter
            width: 64; height: 64
            radius: 32
            color: root.appTheme ? root.appTheme.primaryContainer : "#CCE5FF"
            border.color: root.appTheme ? root.appTheme.border : "#e2e8f0"
            border.width: 1

            Rectangle {
                anchors.centerIn: parent
                width: 24; height: 24
                radius: 4
                color: "transparent"
                border.color: root.appTheme ? root.appTheme.primary : "#006497"
                border.width: 2
            }
        }

        // Title
        Text {
            Layout.alignment: Qt.AlignHCenter
            text: qsTr("未打开工作区")
            font.pixelSize: root.appTheme ? root.appTheme.title : 24
            font.family: root.appTheme ? root.appTheme.fontFamily : "sans-serif"
            font.bold: true
            color: root.appTheme ? root.appTheme.textPrimary : "#E2E2E5"
        }

        // Description
        Text {
            Layout.alignment: Qt.AlignHCenter
            text: qsTr("选择或创建工作区后开始写作")
            font.pixelSize: root.appTheme ? root.appTheme.body : 14
            font.family: root.appTheme ? root.appTheme.fontFamily : "sans-serif"
            color: root.appTheme ? root.appTheme.textSecondary : "#8C9198"
            horizontalAlignment: Text.AlignHCenter
            Layout.fillWidth: true
            wrapMode: Text.Wrap
        }

        // Actions
        ColumnLayout {
            Layout.alignment: Qt.AlignHCenter
            Layout.topMargin: root.appTheme ? root.appTheme.sp8 : 8
            spacing: root.appTheme ? root.appTheme.sp12 : 12
            width: 280

            AppButton {
                Layout.fillWidth: true
                text: qsTr("新建工作区")
                theme: root.appTheme
                variant: "primary"
                onClicked: createWorkspaceDialog.open()
            }

            AppButton {
                Layout.fillWidth: true
                text: qsTr("打开工作区")
                theme: root.appTheme
                variant: "secondary"
                onClicked: openWorkspaceDialog.open()
            }


        }
    }
}
