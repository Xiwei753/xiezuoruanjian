import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Rectangle {
    id: root
    property var dt: null
    property var backendRef: null
    property var appState: ({})
    property var tree: []

    signal openProject(string projectId)
    signal createProject()

    color: dt ? dt.bg : "#111318"

    function getProjects() {
        var projects = [];
        if (!tree) return projects;
        for (var i = 0; i < tree.length; i++) {
            if (tree[i].type === "project") {
                projects.push(tree[i]);
            }
        }
        return projects;
    }

    function getProjectWordCount(projectId) {
        var count = 0;
        if (!tree) return count;
        for (var i = 0; i < tree.length; i++) {
            var item = tree[i];
            if (item.type === "chapter" && item.projectId === projectId) {
                count += (item.wordCount || 0);
            }
        }
        return count;
    }

    function getTodayInput(projectId) {
        if (!backendRef) return 0;
        try {
            var today = new Date();
            var dateStr = today.getFullYear() + "-" +
                String(today.getMonth() + 1).padStart(2, '0') + "-" +
                String(today.getDate()).padStart(2, '0');
            var summary = JSON.parse(backendRef.get_writing_stats_summary(dateStr, dateStr));
            if (summary && summary.per_project && summary.per_project[projectId]) {
                return summary.per_project[projectId].human_typed_chars || 0;
            }
        } catch (e) {}
        return 0;
    }

    HubPageFrame {
        anchors.fill: parent
        dt: root.dt

        headerData: [ HubPageHeader {
            dt: root.dt
            title: "作品"
            subtitle: projectModel.count > 0 ? (projectModel.count + " 部作品") : "开始你的创作之旅"

            Rectangle {
                width: newBtnRow.implicitWidth + (dt ? dt.sp24 : 24)
                height: dt ? dt.actionButtonHeight : 40
                radius: dt ? dt.actionButtonRadius : 12
                color: newProjectHover.containsMouse ? (dt ? dt.accentHover : "#8E9EE8") : (dt ? dt.accent : "#7B8CDE")

                Row {
                    id: newBtnRow
                    anchors.centerIn: parent
                    spacing: dt ? dt.sp6 : 6

                    Text {
                        text: "+"
                        color: "#FFFFFF"
                        font.pixelSize: dt ? dt.fontLg : 16
                        font.weight: Font.Bold
                        anchors.verticalCenter: parent.verticalCenter
                    }
                    Text {
                        text: "新建作品"
                        color: "#FFFFFF"
                        font.pixelSize: dt ? dt.fontMd : 14
                        font.weight: Font.Medium
                        anchors.verticalCenter: parent.verticalCenter
                    }
                }

                MouseArea {
                    id: newProjectHover
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.createProject()
                }
            }
        } ]

        // Project grid
        ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            ScrollBar.vertical.policy: ScrollBar.AsNeeded

            GridView {
                id: grid
                anchors.fill: parent
                cellWidth: root.computeCellWidth()
                cellHeight: 214
                leftMargin: 0
                rightMargin: 0
                model: ListModel { id: projectModel }
                delegate: ProjectCard {
                    dt: root.dt
                    width: grid.cellWidth - (dt ? dt.gridGap : 16)
                    height: grid.cellHeight - (dt ? dt.gridGap : 16)
                    projectId: model.projectId
                    title: model.projectTitle
                    wordCount: model.projectWordCount
                    todayInput: model.projectTodayInput
                    lastEdited: model.projectLastEdited
                    syncStatus: model.projectSyncStatus
                    accentColor: model.projectAccent
                    onClicked: root.openProject(model.projectId)
                }

                add: Transition {
                    NumberAnimation { property: "opacity"; from: 0; to: 1; duration: 200 }
                }
            }
        }

        // Empty state
        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: projectModel.count === 0

            ColumnLayout {
                anchors.centerIn: parent
                spacing: dt ? dt.sp16 : 16

                Text {
                    text: "\uD83D\uDCD6"
                    font.pixelSize: 48
                    Layout.alignment: Qt.AlignHCenter
                }

                Text {
                    text: "暂无作品"
                    color: dt ? dt.textPrimary : "#E2E4E9"
                    font.pixelSize: dt ? dt.fontXl : 18
                    font.weight: Font.DemiBold
                    Layout.alignment: Qt.AlignHCenter
                }

                Text {
                    text: "点击「新建作品」开始创作"
                    color: dt ? dt.textSecondary : "#9CA0AB"
                    font.pixelSize: dt ? dt.fontMd : 14
                    Layout.alignment: Qt.AlignHCenter
                }
            }
        }
    }

    function computeCellWidth() {
        var gap = dt ? dt.gridGap : 16;
        var margin = root.width >= 980 ? (dt ? dt.pageMarginWide : 40) : (dt ? dt.pageMarginNarrow : 24);
        var w = root.width - margin * 2;
        var minCardWidth = 280;
        var cols = Math.max(1, Math.floor((w + gap) / (minCardWidth + gap)));
        return Math.floor((w - (cols - 1) * gap) / cols) + gap;
    }

    function refreshProjects() {
        projectModel.clear();
        var projects = getProjects();
        var accentColors = ["#7B8CDE", "#DE8C7B", "#7BDE8C", "#DE7BC4", "#7BC4DE", "#C4DE7B"];
        for (var i = 0; i < projects.length; i++) {
            var p = projects[i];
            var wc = getProjectWordCount(p.id);
            var ti = getTodayInput(p.id);
            projectModel.append({
                projectId: p.id,
                projectTitle: p.title || "未命名作品",
                projectWordCount: wc,
                projectTodayInput: ti,
                projectLastEdited: p.updatedAt || "",
                projectSyncStatus: "none",
                projectAccent: accentColors[i % accentColors.length]
            });
        }
    }

    Component.onCompleted: refreshProjects()
    onTreeChanged: refreshProjects()
}
