// Object target search for unified manual links.
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Rectangle {
    id: root
    required property var dt
    property var searchBackendRef: null
    property string projectId: ""
    property var sourceRef: null
    property var initialTargets: []
    property var results: []
    property var selectedTargets: []
    property string pendingRequestId: ""
    property string errorText: ""
    property bool loading: false

    signal targetsSelected(var targets)
    signal cancelled()

    color: Kirigami.Theme.alternateBackgroundColor

    function startSearch() {
        if (!searchBackendRef || searchField.text.trim().length === 0) {
            results = []
            pendingRequestId = ""
            return
        }
        loading = true
        errorText = ""
        pendingRequestId = searchBackendRef.search(
            searchField.text.trim(), "all", 40, "",
            JSON.stringify({ currentProjectId: projectId }), false)
    }

    function objectRefKindLabel(kind) {
        var labels = ({
            project: qsTr("作品"), volume: qsTr("分卷"), chapter: qsTr("章节"),
            textAnchor: qsTr("正文位置"), namedEntry: qsTr("登记名称"),
            starMap: qsTr("星图"), nodeAsset: qsTr("星图节点"),
            nodeInstance: qsTr("星图节点实例"), embedInstance: qsTr("子星图"),
            starMapNode: qsTr("星图节点"),
            starMapEmbed: qsTr("子星图")
        })
        return labels[kind] || String(kind || "")
    }

    function targetForResult(result) {
        var objectId = String(result && result.objectId ? result.objectId : "")
        var t = result && result.target ? result.target : ({})
        var p = String(t.projectId || projectId || "")
        var entryPrefix = "annotation:" + p + ":entry:"
        if (p && objectId.indexOf(entryPrefix) === 0) {
            var entryId = objectId.substring(entryPrefix.length)
            return { objectRef: { kind: "namedEntry", projectId: p, objectId: entryId } }
        }
        if (t.nodeId && t.starmapId) {
            // 搜索结果带素材 ID 时按共享素材标注；旧结果只有实例 ID，保留旧结构。
            var assetId = String(t.assetId || "")
            if (assetId)
                return { objectRef: { kind: "nodeAsset", projectId: p, objectId: assetId } }
            return { objectRef: { kind: "starMapNode", projectId: p, objectId: String(t.nodeId) } }
        }
        if (t.embedId && t.starmapId)
            return { objectRef: { kind: "starMapEmbed", projectId: p, objectId: String(t.embedId) } }
        if (t.starmapId)
            return { objectRef: { kind: "starMap", projectId: p, objectId: String(t.starmapId) } }
        if (t.chapterId)
            return { objectRef: { kind: "chapter", projectId: p, objectId: String(t.chapterId) } }
        if (t.volumeId)
            return { objectRef: { kind: "volume", projectId: p, objectId: String(t.volumeId) } }
        if (t.projectId)
            return { objectRef: { kind: "project", projectId: p, objectId: String(t.projectId) } }
        return null
    }

    function sameTarget(a, b) {
        return JSON.stringify(a) === JSON.stringify(b)
    }

    function isSelected(target) {
        for (var i = 0; i < selectedTargets.length; ++i)
            if (sameTarget(selectedTargets[i], target)) return true
        return false
    }

    function toggleTarget(target, enabled) {
        var next = selectedTargets.slice(0)
        var index = -1
        for (var i = 0; i < next.length; ++i)
            if (sameTarget(next[i], target)) { index = i; break }
        if (enabled && index < 0) next.push(target)
        if (!enabled && index >= 0) next.splice(index, 1)
        selectedTargets = next
    }

    Connections {
        target: root.searchBackendRef
        ignoreUnknownSignals: true
        function onResults_ready(requestId, pageJson, append, error) {
            if (String(requestId) !== String(root.pendingRequestId)) return
            root.loading = false
            root.pendingRequestId = ""
            if (error && String(error).length) {
                root.errorText = String(error)
                root.results = []
                return
            }
            try {
                var page = JSON.parse(String(pageJson || "{}"))
                root.results = page && page.items ? page.items : []
            } catch (e) {
                root.errorText = qsTr("搜索结果格式异常")
                root.results = []
            }
        }
    }

    onInitialTargetsChanged: selectedTargets = (initialTargets || []).slice(0)
    onSourceRefChanged: selectedTargets = (initialTargets || []).slice(0)

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Kirigami.Units.largeSpacing
        spacing: Kirigami.Units.smallSpacing

        RowLayout {
            Layout.fillWidth: true
            TextField {
                id: searchField
                Layout.fillWidth: true
                placeholderText: qsTr("搜索作品、章节、正文或星图对象")
                onAccepted: root.startSearch()
            }
            Button { text: qsTr("搜索"); onClicked: root.startSearch() }
        }

        Label { text: qsTr("搜索结果"); font.bold: true }
        ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            ListView {
                id: resultsList
                model: root.results
                spacing: 1
                delegate: CheckDelegate {
                    id: resultDelegate
                    required property var modelData
                    readonly property string displayTitle: String(modelData.title || modelData.path || qsTr("对象"))
                    readonly property var resultTarget: root.targetForResult(modelData)
                    readonly property string kindLabel: resultTarget && resultTarget.objectRef
                        ? root.objectRefKindLabel(resultTarget.objectRef.kind) : ""
                    width: resultsList.width
                    text: displayTitle
                    checked: resultTarget ? root.isSelected(resultTarget) : false
                    enabled: resultTarget !== null
                    onToggled: {
                        if (resultTarget) root.toggleTarget(resultTarget, checked)
                    }
                    contentItem: ColumnLayout {
                        spacing: 0
                        Label { text: resultDelegate.displayTitle; elide: Text.ElideRight; Layout.fillWidth: true }
                        Label {
                            text: (resultDelegate.kindLabel ? resultDelegate.kindLabel + " · " : "")
                                  + (modelData.path || modelData.summary || "")
                            color: Kirigami.Theme.disabledTextColor
                            elide: Text.ElideRight
                            font.pointSize: Kirigami.Theme.smallFont.pointSize
                            Layout.fillWidth: true
                        }
                    }
                }
            }
        }

        Label {
            Layout.fillWidth: true
            visible: root.errorText.length > 0
            text: root.errorText
            color: Kirigami.Theme.negativeTextColor
            wrapMode: Text.Wrap
        }

        RowLayout {
            Layout.fillWidth: true
            Label { Layout.fillWidth: true; text: qsTr("已选 %1 个目标").arg(root.selectedTargets.length) }
            Button { text: qsTr("取消"); onClicked: root.cancelled() }
            Button {
                text: qsTr("添加链接")
                highlighted: true
                enabled: root.selectedTargets.length > 0 && root.sourceRef !== null
                onClicked: root.targetsSelected(root.selectedTargets.slice(0))
            }
        }
    }
}
