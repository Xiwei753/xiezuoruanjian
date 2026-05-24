import QtQuick 2.15
import QtQuick.Controls 2.15

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
        var projects = []
        if (!tree) return projects
        for (var i = 0; i < tree.length; i++) {
            if (tree[i].type === "project") projects.push(tree[i])
        }
        return projects
    }

    function getProjectWordCount(projectId) {
        var count = 0
        if (!tree) return count
        for (var i = 0; i < tree.length; i++) {
            var item = tree[i]
            if (item.type === "chapter" && item.projectId === projectId) count += (item.wordCount || 0)
        }
        return count
    }

    function getTodayInput(projectId) {
        if (!backendRef) return 0
        try {
            var today = new Date()
            var dateStr = today.getFullYear() + "-" + String(today.getMonth() + 1).padStart(2, "0") + "-" + String(today.getDate()).padStart(2, "0")
            var summary = JSON.parse(backendRef.get_writing_stats_summary(dateStr, dateStr))
            if (summary && summary.per_project && summary.per_project[projectId]) return summary.per_project[projectId].human_typed_chars || 0
        } catch (e) {}
        return 0
    }

    CardCollectionPage {
        anchors.fill: parent
        dt: root.dt
        title: "作品"
        subtitle: projectModel.count > 0 ? (projectModel.count + " 部作品") : "开始你的创作之旅"
        actionText: "+ 新建作品"
        model: projectModel
        cardHeight: 214
        minCardWidth: 280
        emptyIcon: "📖"
        emptyTitle: "暂无作品"
        emptySubtitle: "点击「新建作品」开始创作"
        onActionClicked: root.createProject()

        delegate: ProjectCard {
            dt: root.dt
            width: 280
            height: 198
            projectId: model.projectId
            title: model.projectTitle
            wordCount: model.projectWordCount
            todayInput: model.projectTodayInput
            lastEdited: model.projectLastEdited
            syncStatus: model.projectSyncStatus
            accentColor: model.projectAccent
            onClicked: root.openProject(model.projectId)
        }
    }

    ListModel { id: projectModel }

    function refreshProjects() {
        projectModel.clear()
        var projects = getProjects()
        var accentColors = ["#7B8CDE", "#DE8C7B", "#7BDE8C", "#DE7BC4", "#7BC4DE", "#C4DE7B"]
        for (var i = 0; i < projects.length; i++) {
            var p = projects[i]
            projectModel.append({
                projectId: p.id,
                projectTitle: p.title || "未命名作品",
                projectWordCount: getProjectWordCount(p.id),
                projectTodayInput: getTodayInput(p.id),
                projectLastEdited: p.updatedAt || "",
                projectSyncStatus: "none",
                projectAccent: accentColors[i % accentColors.length]
            })
        }
    }

    Component.onCompleted: refreshProjects()
    onTreeChanged: refreshProjects()
}
