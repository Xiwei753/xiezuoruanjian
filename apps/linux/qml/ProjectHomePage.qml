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

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: dt ? dt.sp32 : 32
        spacing: 0

        // Header row
        RowLayout {
            Layout.fillWidth: true
            spacing: dt ? dt.sp16 : 16

            Text {
                text: "作品"
                color: dt ? dt.textPrimary : "#E2E4E9"
                font.pixelSize: dt ? dt.fontTitle : 26
                font.weight: Font.Bold
            }

            Item { Layout.fillWidth: true }

            // New project button
            Rectangle {
                width: newBtnRow.implicitWidth + (dt ? dt.sp24 : 24)
                height: 36
                radius: dt ? dt.radiusMd : 12
                color: dt ? dt.accent : "#7B8CDE"

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
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.createProject()
                }
            }
        }

        // Subtitle
        Text {
            Layout.fillWidth: true
            Layout.topMargin: dt ? dt.sp8 : 8
            text: {
                var projects = getProjects();
                if (projects.length === 0) return "开始你的创作之旅";
                return projects.length + " 部作品";
            }
            color: dt ? dt.textSecondary : "#9CA0AB"
            font.pixelSize: dt ? dt.fontMd : 14
        }

        // Spacer
        Item { Layout.preferredHeight: dt ? dt.sp24 : 24 }

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
                cellHeight: 210
                model: ListModel { id: projectModel }
                delegate: ProjectCard {
                    dt: root.dt
                    width: grid.cellWidth - dt.sp16
                    height: grid.cellHeight - dt.sp16
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
        ColumnLayout {
            anchors.centerIn: parent
            spacing: dt ? dt.sp16 : 16
            visible: projectModel.count === 0

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

    function computeCellWidth() {
        var w = root.width - (dt ? dt.sp64 : 64);
        if (w < 300) return Math.max(w, 200);
        if (w < 600) return Math.floor(w / 2);
        if (w < 900) return Math.floor(w / 3);
        if (w < 1200) return Math.floor(w / 4);
        return Math.floor(w / 5);
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
