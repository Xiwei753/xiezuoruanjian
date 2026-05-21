import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Item {
    id: root
    property var theme: null
    property var backendRef: null

    signal createWorkspace()
    signal openWorkspace()
    signal initFromGithub()

    Rectangle {
        anchors.fill: parent
        color: root.theme ? root.theme.bg : "#f5f5f5"
    }

    ColumnLayout {
        anchors.centerIn: parent
        spacing: root.theme ? root.theme.sp24 : 24
        width: Math.min(parent.width - 80, 480)

        // Icon
        Rectangle {
            Layout.alignment: Qt.AlignHCenter
            width: 64; height: 64
            radius: 32
            color: root.theme ? root.theme.surface : "#ffffff"
            border.color: root.theme ? root.theme.border : "#e2e8f0"
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
            font.pixelSize: root.theme ? root.theme.fontXxl : 22
            font.weight: Font.Bold
            color: root.theme ? root.theme.text : "#0f172a"
        }

        // Description
        Label {
            Layout.alignment: Qt.AlignHCenter
            text: "选择或创建工作区后开始写作"
            font.pixelSize: root.theme ? root.theme.fontMd : 13
            color: root.theme ? root.theme.textDim : "#64748b"
            horizontalAlignment: Text.AlignHCenter
            Layout.fillWidth: true
            wrapMode: Text.Wrap
        }

        // Actions
        ColumnLayout {
            Layout.alignment: Qt.AlignHCenter
            Layout.topMargin: root.theme ? root.theme.sp8 : 8
            spacing: root.theme ? root.theme.sp12 : 12
            width: 280

            AppButton {
                Layout.fillWidth: true
                text: "新建工作区"
                theme: root.theme
                onClicked: root.createWorkspace()
            }

            AppButton {
                Layout.fillWidth: true
                text: "打开工作区"
                theme: root.theme
                onClicked: root.openWorkspace()
            }

            Rectangle {
                Layout.fillWidth: true
                height: 1
                color: root.theme ? root.theme.divider : "#e2e8f0"
                Layout.topMargin: root.theme ? root.theme.sp4 : 4
                Layout.bottomMargin: root.theme ? root.theme.sp4 : 4
            }

            Button {
                Layout.fillWidth: true
                flat: true
                text: "从 GitHub 同步仓库初始化"
                implicitHeight: 36
                onClicked: root.initFromGithub()

                contentItem: Text {
                    text: parent.text
                    color: root.theme ? root.theme.primary : "#0ea5e9"
                    font.pixelSize: root.theme ? root.theme.fontMd : 13
                    font.weight: Font.Medium
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }

                background: Rectangle {
                    color: parent.hovered ? (root.theme ? root.theme.hover : "#f1f5f9") : "transparent"
                    radius: root.theme ? root.theme.radiusSm : 4
                }
            }
        }
    }
}
