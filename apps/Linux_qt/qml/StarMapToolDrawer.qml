import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Rectangle {
    id: root
    required property var dt
    property string selectedTool: "tags"
    property bool isOpen: false
    property bool aiAvailable: false
    property bool aiEnabled: false
    property var annotationBackendRef: null
    property var searchBackendRef: null
    // Issue #1016：星图右栏复用写作区同一套 AI 会话。
    property var agentCliBackendRef: null
    property var agentRuntimeBackendRef: null
    property string workspaceProjectId: ""
    property string workspaceProjectTitle: ""
    property string aiContextLabel: ""
    property string projectId: ""
    property var targetRef: null
    property string targetLabel: ""

    signal closeRequested()
    signal targetActivated(var target)
    signal linkPickerRequested(var sourceRef, var link)

    function refresh() {
        tagsPanel.refresh()
    }

    visible: isOpen
    clip: true
    color: Kirigami.Theme.alternateBackgroundColor

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        RowLayout {
            Layout.fillWidth: true
            Layout.preferredHeight: Kirigami.Units.gridUnit * 2.4
            anchors.leftMargin: Kirigami.Units.smallSpacing
            anchors.rightMargin: Kirigami.Units.smallSpacing
            Label {
                Layout.fillWidth: true
                text: root.selectedTool === "ai" ? qsTr("AI") : qsTr("标签与引用")
                font.bold: true
                elide: Text.ElideRight
            }
            ToolButton {
                icon.name: "window-close"
                display: AbstractButton.IconOnly
                onClicked: root.closeRequested()
            }
        }
        Rectangle { Layout.fillWidth: true; height: 1; color: Kirigami.Theme.disabledTextColor; opacity: 0.35 }

        TagsPanel {
            id: tagsPanel
            visible: root.selectedTool === "tags"
            Layout.fillWidth: true
            Layout.fillHeight: true
            dt: root.dt
            annotationBackendRef: root.annotationBackendRef
            searchBackendRef: root.searchBackendRef
            projectId: root.projectId
            targetRef: root.targetRef
            targetLabel: root.targetLabel
            onTargetActivated: function(target) { root.targetActivated(target) }
            onLinkPickerRequested: function(sourceRef, link) { root.linkPickerRequested(sourceRef, link) }
        }

        AiToolPanel {
            visible: root.selectedTool === "ai"
            Layout.fillWidth: true
            Layout.fillHeight: true
            dt: root.dt
            agentCliBackendRef: root.agentCliBackendRef
            runtimeBackendRef: root.agentRuntimeBackendRef
            workspaceProjectId: root.workspaceProjectId
            workspaceProjectTitle: root.workspaceProjectTitle
            contextLabel: root.aiContextLabel
        }
    }
}
