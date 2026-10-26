// Writing editor context actions for clipboard and shared annotations.
import QtQuick
import QtQuick.Controls

Menu {
    id: editorContextMenu

    property var editorItem: null
    property var editorControllerRef: null
    property var annotationBackendRef: null
    property var searchBackendRef: null
    property var pendingSourceRef: null
    property string pendingLinkId: ""
    property var pendingLinkTarget: null
    required property var dt

    function parseData(raw) {
        try {
            var envelope = JSON.parse(String(raw || "{}"))
            return envelope && envelope.success === true ? envelope.data : null
        } catch (e) { return null }
    }

    function sourceForAction() {
        var controller = editorControllerRef
        if (!annotationBackendRef || !controller || !controller.projectId || !controller.chapterId)
            return null
        if (editorItem && editorItem.has_selection
                && Number(editorItem.selection_end_byte) > Number(editorItem.selection_start_byte)) {
            var currentText = controller.getEditorPlainText()
            var revision = Number(editorItem.text_revision)
            if (annotationBackendRef.get_chapter_annotations_json) {
                var currentSnapshot = controller.annotationNavigationSnapshot
                var baselineId = currentSnapshot
                        && String(currentSnapshot.projectId) === String(controller.projectId)
                        && String(currentSnapshot.chapterId) === String(controller.chapterId)
                        ? String(currentSnapshot.baselineId || "") : ""
                var snapshot = parseData(annotationBackendRef.get_chapter_annotations_json(
                    controller.projectId, controller.chapterId, revision, currentText, baselineId))
                var anchors = snapshot && snapshot.anchors ? snapshot.anchors : []
                for (var i = 0; i < anchors.length; ++i) {
                    if (Number(anchors[i].startByte) === Number(editorItem.selection_start_byte)
                            && Number(anchors[i].endByte) === Number(editorItem.selection_end_byte))
                        return { kind: "textAnchor", projectId: controller.projectId,
                            objectId: String(anchors[i].anchorId) }
                }
            }
            var anchor = parseData(annotationBackendRef.create_text_anchor_json(
                controller.projectId, controller.chapterId,
                Number(editorItem.selection_start_byte), Number(editorItem.selection_end_byte),
                revision, currentText))
            if (!anchor || !anchor.anchorId) return null
            return { kind: "textAnchor", projectId: controller.projectId, objectId: String(anchor.anchorId) }
        }
        return { kind: "chapter", projectId: controller.projectId, objectId: controller.chapterId }
    }

    function openTags() {
        pendingSourceRef = sourceForAction()
        if (pendingSourceRef) annotationTagsPopup.open()
    }

    function openLinkPicker(link) {
        pendingSourceRef = sourceForAction()
        pendingLinkId = link ? String(link.linkId || "") : ""
        pendingLinkTarget = link ? link.target : null
        if (pendingSourceRef) annotationLinkPopup.open()
    }

    MenuItem {
        text: qsTr("复制")
        icon.name: "edit-copy"
        enabled: editorContextMenu.editorItem && editorContextMenu.editorItem.has_selection
        onTriggered: if (editorContextMenu.editorItem) editorContextMenu.editorItem.clipboard_copy()
    }
    MenuItem {
        text: qsTr("粘贴")
        icon.name: "edit-paste"
        enabled: editorContextMenu.editorItem && editorContextMenu.editorItem.editor_enabled
        onTriggered: if (editorContextMenu.editorItem) editorContextMenu.editorItem.clipboard_paste()
    }
    MenuItem {
        text: qsTr("全选")
        icon.name: "edit-select-all"
        enabled: editorContextMenu.editorItem && editorContextMenu.editorItem.editor_enabled
        onTriggered: if (editorContextMenu.editorItem) editorContextMenu.editorItem.select_all()
    }
    MenuItem {
        text: qsTr("删除")
        icon.name: "edit-delete"
        enabled: editorContextMenu.editorItem && editorContextMenu.editorItem.has_selection
        onTriggered: if (editorContextMenu.editorItem) editorContextMenu.editorItem.delete_selection()
    }

    MenuSeparator {}
    MenuItem {
        text: qsTr("标签")
        icon.name: "tag"
        enabled: !!editorContextMenu.annotationBackendRef
        onTriggered: editorContextMenu.openTags()
    }
    MenuItem {
        text: qsTr("添加超链接")
        icon.name: "insert-link"
        enabled: !!editorContextMenu.annotationBackendRef && !!editorContextMenu.searchBackendRef
        onTriggered: editorContextMenu.openLinkPicker(null)
    }
    MenuItem {
        text: qsTr("登记名称")
        icon.name: "contact-new"
        enabled: !!editorContextMenu.editorItem && editorContextMenu.editorItem.has_selection
                 && String(editorContextMenu.editorItem.selected_text()).trim().length > 0
                 && !!editorContextMenu.annotationBackendRef
        onTriggered: {
            editorContextMenu.pendingSourceRef = editorContextMenu.sourceForAction()
            if (editorContextMenu.pendingSourceRef) {
                nameField.text = String(editorContextMenu.editorItem.selected_text()).trim()
                registerNameDialog.open()
            }
        }
    }
    MenuItem {
        text: qsTr("查看引用")
        icon.name: "view-list-details"
        enabled: !!editorContextMenu.annotationBackendRef
        onTriggered: editorContextMenu.openTags()
    }

    Popup {
        id: annotationTagsPopup
        modal: true
        anchors.centerIn: Overlay.overlay
        width: Math.min(420, Overlay.overlay ? Overlay.overlay.width - 32 : 420)
        height: Math.min(680, Overlay.overlay ? Overlay.overlay.height - 32 : 680)
        padding: 0
        contentItem: TagsPanel {
            dt: editorContextMenu.dt
            annotationBackendRef: editorContextMenu.annotationBackendRef
            searchBackendRef: editorContextMenu.searchBackendRef
            projectId: editorContextMenu.editorControllerRef ? editorContextMenu.editorControllerRef.projectId : ""
            chapterId: editorContextMenu.editorControllerRef ? editorContextMenu.editorControllerRef.chapterId : ""
            baselineId: editorContextMenu.editorControllerRef
                && editorContextMenu.editorControllerRef.annotationNavigationSnapshot
                ? String(editorContextMenu.editorControllerRef.annotationNavigationSnapshot.baselineId || "") : ""
            targetRef: editorContextMenu.pendingSourceRef
            targetLabel: targetRef && targetRef.kind === "textAnchor" ? qsTr("所选正文") : qsTr("本章")
            onTargetActivated: function(target) {
                if (target && target.externalUrl) Qt.openUrlExternally(String(target.externalUrl))
                else if (target) editorContextMenu.editorControllerRef.annotationTargetActivated(target)
            }
            onLinkPickerRequested: function(sourceRef, link) {
                editorContextMenu.pendingSourceRef = sourceRef
                editorContextMenu.pendingLinkId = link ? String(link.linkId || "") : ""
                editorContextMenu.pendingLinkTarget = link ? link.target : null
                annotationLinkPopup.open()
            }
            onChanged: {
                if (editorContextMenu.editorControllerRef && editorContextMenu.editorControllerRef.refreshChapterAnnotations)
                    editorContextMenu.editorControllerRef.refreshChapterAnnotations()
            }
        }
    }

    Popup {
        id: annotationLinkPopup
        modal: true
        anchors.centerIn: Overlay.overlay
        width: Math.min(620, Overlay.overlay ? Overlay.overlay.width - 48 : 620)
        height: Math.min(560, Overlay.overlay ? Overlay.overlay.height - 48 : 560)
        padding: 0
        contentItem: AnnotationLinkPicker {
            dt: editorContextMenu.dt
            searchBackendRef: editorContextMenu.searchBackendRef
            projectId: editorContextMenu.pendingSourceRef ? editorContextMenu.pendingSourceRef.projectId : ""
            sourceRef: editorContextMenu.pendingSourceRef
            initialTargets: editorContextMenu.pendingLinkTarget ? [editorContextMenu.pendingLinkTarget] : []
            onCancelled: annotationLinkPopup.close()
            onTargetsSelected: function(targets) {
                if (!editorContextMenu.annotationBackendRef || !editorContextMenu.pendingSourceRef) return
                for (var i = 0; i < targets.length; ++i) {
                    if (i === 0 && editorContextMenu.pendingLinkId) {
                        editorContextMenu.annotationBackendRef.update_link_json(
                            String(editorContextMenu.pendingSourceRef.projectId), editorContextMenu.pendingLinkId,
                            JSON.stringify(editorContextMenu.pendingSourceRef), JSON.stringify(targets[i]))
                    } else {
                        editorContextMenu.annotationBackendRef.create_link_json(
                            JSON.stringify(editorContextMenu.pendingSourceRef), JSON.stringify(targets[i]))
                    }
                }
                editorContextMenu.pendingLinkId = ""
                editorContextMenu.pendingLinkTarget = null
                annotationLinkPopup.close()
                annotationTagsPopup.contentItem.refresh()
                if (editorContextMenu.editorControllerRef && editorContextMenu.editorControllerRef.refreshChapterAnnotations)
                    editorContextMenu.editorControllerRef.refreshChapterAnnotations()
            }
        }
    }

    Dialog {
        id: registerNameDialog
        title: qsTr("登记名称")
        modal: true
        anchors.centerIn: Overlay.overlay
        standardButtons: Dialog.Ok | Dialog.Cancel
        contentItem: TextField { id: nameField; placeholderText: qsTr("名称") }
        onAccepted: {
            if (!editorContextMenu.annotationBackendRef || !editorContextMenu.pendingSourceRef || !nameField.text.trim())
                return
            var raw = editorContextMenu.annotationBackendRef.create_named_entry_json(
                String(editorContextMenu.pendingSourceRef.projectId), nameField.text.trim(), "[]", "[]", true)
            var entry = editorContextMenu.parseData(raw)
            if (!entry || !entry.entryId) return
            var target = { objectRef: {
                kind: "namedEntry", projectId: String(editorContextMenu.pendingSourceRef.projectId),
                objectId: String(entry.entryId)
            }, externalUrl: null }
            editorContextMenu.annotationBackendRef.create_link_json(
                JSON.stringify(editorContextMenu.pendingSourceRef), JSON.stringify(target))
            if (editorContextMenu.editorControllerRef && editorContextMenu.editorControllerRef.refreshChapterAnnotations)
                editorContextMenu.editorControllerRef.refreshChapterAnnotations()
        }
    }
}
