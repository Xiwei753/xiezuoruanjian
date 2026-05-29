// =============================================================================
// EmptyWorkspace.qml — 空工作区引导页
// =============================================================================
//
// 层级：Linux UI 层（QML UI 组件）
// 职责：工作区为空时的引导页面，提示用户创建工作区
// 约束：
//   - 纯展示组件，创建操作通过 signal 传递给 main.qml
//   - 使用 DesignTokens 统一样式
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: root
    property var appTheme: null
    property var backendRef: null

    signal createWorkspace()
    signal openWorkspace()
    signal initFromGithub()

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
            color: root.appTheme ? root.appTheme.surface : "#ffffff"
            border.color: root.appTheme ? root.appTheme.border : "#e2e8f0"
            border.width: 1

            Text {
                anchors.centerIn: parent
                text: "\uD83D\uDCD6"
                font.pixelSize: 28
            }
        }

        // Title
        Label {
            Layout.alignment: Qt.AlignHCenter
            text: "未打开工作区"
            font.pixelSize: root.appTheme ? root.appTheme.fontXxl : 22
            font.weight: Font.Bold
            color: root.appTheme ? root.appTheme.textPrimary : "#0f172a"
        }

        // Description
        Label {
            Layout.alignment: Qt.AlignHCenter
            text: "选择或创建工作区后开始写作"
            font.pixelSize: root.appTheme ? root.appTheme.fontMd : 13
            color: root.appTheme ? root.appTheme.textSecondary : "#475569"
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
                text: "新建工作区"
                theme: root.appTheme
                onClicked: root.createWorkspace()
            }

            AppButton {
                Layout.fillWidth: true
                text: "打开工作区"
                theme: root.appTheme
                onClicked: root.openWorkspace()
            }

            Rectangle {
                Layout.fillWidth: true
                height: 1
                color: root.appTheme ? root.appTheme.divider : "#e2e8f0"
                Layout.topMargin: root.appTheme ? root.appTheme.sp4 : 4
                Layout.bottomMargin: root.appTheme ? root.appTheme.sp4 : 4
            }

            Button {
                Layout.fillWidth: true
                flat: true
                text: "从 GitHub 同步仓库初始化"
                implicitHeight: 36
                onClicked: {
                    root.initFromGithub()
                }

                contentItem: Text {
                    text: parent.text
                    color: root.appTheme ? root.appTheme.primary : "#3b82f6"
                    font.pixelSize: root.appTheme ? root.appTheme.fontMd : 13
                    font.weight: Font.Medium
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }

                background: Rectangle {
                    color: parent.hovered ? (root.appTheme ? root.appTheme.hover : "#f1f5f9") : "transparent"
                    radius: root.appTheme ? root.appTheme.radiusSm : 6
                }
            }

            Label {
                Layout.fillWidth: true
                text: "点击后会进入同步页面：配置远端仓库地址、分支、Token，然后点击「选择目录并初始化/克隆」。"
                font.pixelSize: root.appTheme ? root.appTheme.fontXs : 11
                color: root.appTheme ? root.appTheme.textSecondary : "#475569"
                wrapMode: Text.Wrap
                horizontalAlignment: Text.AlignHCenter
            }
        }
    }
}
