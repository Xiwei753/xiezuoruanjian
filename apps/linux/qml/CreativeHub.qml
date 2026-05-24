import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Rectangle {
    id: root
    property var dt: null
    property var backendRef: null
    property var appState: ({})
    property var tree: []
    property int currentTab: 0
    property bool aiCapable: false
    property bool aiEnabled: false

    signal openProject(string projectId, string projectTitle)
    signal createProject()
    signal openSettings()
    signal openSync()
    signal switchWorkspace()

    color: dt ? dt.bg : "#111318"

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // Top navigation bar
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 52
            color: dt ? dt.surface : "#1A1D23"
            border.color: dt ? dt.border : "#2A2E36"
            border.width: 1

            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: dt ? dt.sp24 : 24
                anchors.rightMargin: dt ? dt.sp24 : 24
                spacing: dt ? dt.sp32 : 32

                // Logo
                Row {
                    spacing: dt ? dt.sp10 : 10
                    Text {
                        text: "Writer"
                        color: dt ? dt.accent : "#7B8CDE"
                        font.pixelSize: dt ? dt.fontXl : 18
                        font.weight: Font.Bold
                        anchors.verticalCenter: parent.verticalCenter
                    }
                }

                // Navigation tabs
                Row {
                    spacing: dt ? dt.sp4 : 4
                    anchors.verticalCenter: parent.verticalCenter

                    Repeater {
                        model: [
                            { label: "作品", idx: 0 },
                            { label: "星图", idx: 1 },
                            { label: "统计", idx: 2 }
                        ]

                        Rectangle {
                            width: navLabel.implicitWidth + (dt ? dt.sp20 : 20)
                            height: 32
                            radius: dt ? dt.radiusSm : 8
                            color: root.currentTab === modelData.idx ?
                                   (dt ? dt.accentSoft : "rgba(123,140,222,0.12)") :
                                   navHover.containsMouse ? (dt ? dt.card : "#1E2128") : "transparent"

                            Text {
                                id: navLabel
                                anchors.centerIn: parent
                                text: modelData.label
                                color: root.currentTab === modelData.idx ?
                                       (dt ? dt.accentText : "#3D4D9E") :
                                       (dt ? dt.textSecondary : "#5C6070")
                                font.pixelSize: dt ? dt.fontMd : 14
                                font.weight: root.currentTab === modelData.idx ? Font.DemiBold : Font.Normal
                            }

                            MouseArea {
                                id: navHover
                                anchors.fill: parent
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: root.currentTab = modelData.idx
                            }
                        }
                    }
                }

                Item { Layout.fillWidth: true }

                // Right actions
                Row {
                    spacing: dt ? dt.sp8 : 8
                    anchors.verticalCenter: parent.verticalCenter

                    // Sync status indicator
                    Rectangle {
                        width: 28; height: 28
                        radius: dt ? dt.radiusSm : 8
                        color: syncHover.containsMouse ? (dt ? dt.card : "#1E2128") : "transparent"
                        visible: root.appState && root.appState.sync && root.appState.sync.status !== "not_configured"

                        Rectangle {
                            anchors.centerIn: parent
                            width: 8; height: 8; radius: 4
                            color: {
                                var s = root.appState && root.appState.sync ? root.appState.sync.status : "none";
                                if (s === "success") return dt ? dt.success : "#5CB880";
                                if (s === "syncing") return dt ? dt.warning : "#E0A840";
                                if (s === "error" || s === "conflict") return dt ? dt.danger : "#E06060";
                                return dt ? dt.textMuted : "#606470";
                            }
                        }

                        MouseArea {
                            id: syncHover
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.openSync()
                        }
                    }

                    // Settings button
                    Rectangle {
                        width: 28; height: 28
                        radius: dt ? dt.radiusSm : 8
                        color: settingsHover.containsMouse ? (dt ? dt.card : "#1E2128") : "transparent"

                        Text {
                            anchors.centerIn: parent
                            text: "\u2699"
                            color: dt ? dt.textSecondary : "#5C6070"
                            font.pixelSize: dt ? dt.fontLg : 16
                        }

                        MouseArea {
                            id: settingsHover
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.openSettings()
                        }
                    }

                    // Workspace switch
                    Rectangle {
                        width: switchRow.implicitWidth + (dt ? dt.sp12 : 12)
                        height: 28
                        radius: dt ? dt.radiusSm : 8
                        color: switchHover.containsMouse ? (dt ? dt.card : "#1E2128") : "transparent"

                        Row {
                            id: switchRow
                            anchors.centerIn: parent
                            spacing: dt ? dt.sp4 : 4
                            Text {
                                text: "\uD83D\uDCC2"
                                font.pixelSize: dt ? dt.fontSm : 12
                                anchors.verticalCenter: parent.verticalCenter
                            }
                            Text {
                                text: "切换"
                                color: dt ? dt.textSecondary : "#5C6070"
                                font.pixelSize: dt ? dt.fontXs : 11
                                anchors.verticalCenter: parent.verticalCenter
                            }
                        }

                        MouseArea {
                            id: switchHover
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.switchWorkspace()
                        }
                    }
                }
            }
        }

        // Content
        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true

            ProjectHomePage {
                dt: root.dt
                backendRef: root.backendRef
                appState: root.appState
                tree: root.tree
                visible: root.currentTab === 0
                anchors.fill: parent
                onOpenProject: function(projectId) {
                    var title = "";
                    for (var i = 0; i < root.tree.length; i++) {
                        if (root.tree[i].id === projectId) {
                            title = root.tree[i].title;
                            break;
                        }
                    }
                    root.openProject(projectId, title);
                }
                onCreateProject: root.createProject()
            }

            StarMapPreviewPage {
                dt: root.dt
                backendRef: root.backendRef
                appState: root.appState
                visible: root.currentTab === 1
                anchors.fill: parent
            }

            StatsPreviewPage {
                dt: root.dt
                backendRef: root.backendRef
                appState: root.appState
                visible: root.currentTab === 2
                anchors.fill: parent
            }
        }
    }
}
