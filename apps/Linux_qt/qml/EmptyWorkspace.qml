// =============================================================================
// EmptyWorkspace.qml — 空工作区引导页
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：工作区为空时的引导页面，提示用户创建工作区
// 约束：
//   - 纯展示组件，创建操作通过 signal 传递给 main.qml
// Issue #900：视觉迁移到 KDE/Breeze（Controls.Label / Controls.Button /
// Kirigami.Heading / Kirigami.Icon），颜色与间距统一取 Kirigami.Theme /
// Kirigami.Units，不再读 DesignTokens 的 M3 字段。
// 公共 API（workspaceBackendRef / 三个信号）保持不变。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Qt.labs.platform as Platform
import org.kde.kirigami as Kirigami

Item {
    id: root
    required property var dt

    property var workspaceBackendRef: null

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
        color: Kirigami.Theme.backgroundColor
    }

    ColumnLayout {
        spacing: Kirigami.Units.largeSpacing
        width: Math.min(parent.width - 80, 480)
        height: implicitHeight
        x: Math.max(0, Math.floor((parent.width - width) / 2))
        y: Math.max(0, Math.floor((parent.height - height) / 2))

        // Icon
        Kirigami.Icon {
            Layout.alignment: Qt.AlignHCenter
            source: "folder-open"
            implicitWidth: Kirigami.Units.iconSizes.enormous
            implicitHeight: Kirigami.Units.iconSizes.enormous
        }

        // Title
        Kirigami.Heading {
            Layout.alignment: Qt.AlignHCenter
            level: 1
            text: qsTr("未打开工作区")
        }

        // Description
        Label {
            Layout.alignment: Qt.AlignHCenter
            text: qsTr("选择或创建工作区后开始写作")
            color: Kirigami.Theme.disabledTextColor
            horizontalAlignment: Text.AlignHCenter
            Layout.fillWidth: true
            wrapMode: Text.Wrap
        }

        // Actions
        ColumnLayout {
            Layout.alignment: Qt.AlignHCenter
            Layout.topMargin: Kirigami.Units.smallSpacing
            spacing: Kirigami.Units.smallSpacing
            Layout.preferredWidth: 280

            Button {
                Layout.fillWidth: true
                text: qsTr("新建工作区")
                highlighted: true
                onClicked: createWorkspaceDialog.open()
            }

            Button {
                Layout.fillWidth: true
                text: qsTr("打开工作区")
                onClicked: openWorkspaceDialog.open()
            }
        }
    }
}
