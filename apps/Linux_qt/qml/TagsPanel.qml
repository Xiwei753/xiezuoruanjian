// Shared flat tag and reference panel for chapter text and StarMap objects.
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Rectangle {
    id: root
    required property var dt
    property var annotationBackendRef: null
    property var searchBackendRef: null
    property string projectId: ""
    property var targetRef: null
    property string targetLabel: ""
    property string chapterId: ""
    property double textRevision: 0
    property string chapterText: ""
    property string baselineId: ""
    property int selectionStartByte: -1
    property int selectionEndByte: -1
    property bool allowSelectionAnchor: false
    property string selectionAnchorKey: ""
    property var selectionAnchorRef: null

    property var tags: []
    property var namedEntries: []
    property var boundTagIds: []
    property var draftTagIds: []
    property string activeTagId: ""
    property string activeEntryId: ""
    property var outgoingLinks: []
    property var backlinks: []
    property string errorText: ""
    property bool busy: false

    signal changed()
    signal targetActivated(var target)
    signal linkPickerRequested(var sourceRef, var link)

    color: Kirigami.Theme.alternateBackgroundColor

    function parseData(raw) {
        try {
            var envelope = JSON.parse(String(raw || "{}"))
            if (!envelope || envelope.success !== true) {
                errorText = envelope && envelope.rawError ? String(envelope.rawError) : qsTr("操作失败")
                return null
            }
            errorText = ""
            return envelope.data
        } catch (e) {
            errorText = qsTr("后端响应无法读取")
            return null
        }
    }

    function effectiveTargetRef(createIfMissing) {
        if (!allowSelectionAnchor || !targetRef || targetRef.kind !== "chapter"
                || selectionStartByte < 0 || selectionEndByte <= selectionStartByte
                || !annotationBackendRef)
            return targetRef
        var key = projectId + ":" + chapterId + ":" + textRevision + ":"
                + selectionStartByte + ":" + selectionEndByte
        if (selectionAnchorRef && selectionAnchorKey === key)
            return selectionAnchorRef
        if (annotationBackendRef.get_chapter_annotations_json) {
            var snapshot = parseData(annotationBackendRef.get_chapter_annotations_json(
                projectId, chapterId, Number(textRevision), chapterText, baselineId))
            var anchors = snapshot && snapshot.anchors ? snapshot.anchors : []
            for (var i = 0; i < anchors.length; ++i) {
                if (Number(anchors[i].startByte) === selectionStartByte
                        && Number(anchors[i].endByte) === selectionEndByte) {
                    selectionAnchorKey = key
                    selectionAnchorRef = {
                        kind: "textAnchor", projectId: projectId,
                        objectId: String(anchors[i].anchorId)
                    }
                    return selectionAnchorRef
                }
            }
        }
        if (!createIfMissing) return null
        var raw = annotationBackendRef.create_text_anchor_json(
            projectId, chapterId, selectionStartByte, selectionEndByte,
            Number(textRevision), chapterText)
        var anchor = parseData(raw)
        if (!anchor || !anchor.anchorId) return targetRef
        selectionAnchorKey = key
        selectionAnchorRef = {
            kind: "textAnchor", projectId: projectId, objectId: String(anchor.anchorId)
        }
        return selectionAnchorRef
    }

    function refresh() {
        var activeTarget = effectiveTargetRef(false)
        if (!annotationBackendRef || !projectId || !targetRef) {
            tags = []
            namedEntries = []
            boundTagIds = []
            outgoingLinks = []
            backlinks = []
            return
        }
        busy = true
        var tagData = parseData(annotationBackendRef.list_tags_json(projectId))
        if (tagData !== null) tags = tagData
        var entryData = parseData(annotationBackendRef.list_named_entries_json(projectId))
        if (entryData !== null) namedEntries = entryData
        if (!activeTarget) {
            boundTagIds = []
            draftTagIds = []
            outgoingLinks = []
            backlinks = []
            busy = false
            return
        }
        var targetJson = JSON.stringify(activeTarget)
        var bindingData = parseData(annotationBackendRef.list_bindings_for_target_json(projectId, targetJson))
        if (bindingData !== null) {
            var ids = []
            for (var i = 0; i < bindingData.length; ++i)
                if (bindingData[i] && !bindingData[i].deletedAtMs) ids.push(String(bindingData[i].tagId))
            boundTagIds = ids
            draftTagIds = ids.slice(0)
        }
        if (annotationBackendRef.list_links_for_source_json) {
            var linkData = parseData(annotationBackendRef.list_links_for_source_json(targetJson))
            if (linkData !== null) outgoingLinks = linkData
        }
        var backlinkData = parseData(annotationBackendRef.find_backlinks_json(
            JSON.stringify({ objectRef: targetRef, externalUrl: null })))
        if (backlinkData !== null) backlinks = backlinkData
        busy = false
    }

    function containsId(list, id) {
        for (var i = 0; i < list.length; ++i)
            if (String(list[i]) === String(id)) return true
        return false
    }

    function toggleDraftTag(id, enabled) {
        var result = draftTagIds.slice(0)
        var index = result.indexOf(id)
        if (enabled && index < 0) result.push(id)
        if (!enabled && index >= 0) result.splice(index, 1)
        draftTagIds = result
    }

    function applyTags() {
        var activeTarget = effectiveTargetRef(true)
        if (!annotationBackendRef || !activeTarget) return
        var result = parseData(annotationBackendRef.set_tag_bindings_json(
            projectId, JSON.stringify(activeTarget), JSON.stringify(draftTagIds)))
        if (result !== null) {
            refresh()
            changed()
        }
    }

    function openLinkPicker(link) {
        var activeTarget = effectiveTargetRef(true)
        if (activeTarget) linkPickerRequested(activeTarget, link || null)
    }

    function deleteLink(link) {
        if (!annotationBackendRef || !link || !link.linkId) return
        var result = parseData(annotationBackendRef.delete_link_json(projectId, String(link.linkId)))
        if (result !== null) {
            refresh()
            changed()
        }
    }

    function editEntry(entry) {
        if (!entry) return
        activeEntryId = String(entry.entryId || "")
        entryNameField.text = String(entry.canonicalName || "")
        entryAliasesField.text = (entry.aliases || []).join(", ")
        entryMisspellingsField.text = (entry.confirmedMisspellings || []).join(", ")
        entryAutoLink.checked = entry.autoLinkEnabled !== false
        entryDialog.open()
    }

    function splitNames(value) {
        var items = String(value || "").split(",")
        var result = []
        for (var i = 0; i < items.length; ++i) {
            var item = items[i].trim()
            if (item && result.indexOf(item) < 0) result.push(item)
        }
        return result
    }

    function saveEntry() {
        var canonical = entryNameField.text.trim()
        if (!annotationBackendRef || !projectId || !canonical) return
        var aliases = JSON.stringify(splitNames(entryAliasesField.text))
        var misspellings = JSON.stringify(splitNames(entryMisspellingsField.text))
        var raw = activeEntryId
            ? annotationBackendRef.update_named_entry_json(projectId, activeEntryId, canonical,
                aliases, misspellings, entryAutoLink.checked)
            : annotationBackendRef.create_named_entry_json(projectId, canonical,
                aliases, misspellings, entryAutoLink.checked)
        if (parseData(raw) !== null) {
            refresh()
            changed()
        }
    }

    function deleteEntry(entry) {
        if (!annotationBackendRef || !entry || !entry.entryId) return
        if (parseData(annotationBackendRef.delete_named_entry_json(projectId, String(entry.entryId))) !== null) {
            refresh()
            changed()
        }
    }

    function createTag() {
        var name = newTagField.text.trim()
        if (!name || !annotationBackendRef) return
        var result = parseData(annotationBackendRef.create_tag_json(projectId, name, "null"))
        if (result !== null) {
            newTagField.text = ""
            refresh()
        }
    }

    function renameActiveTag(name) {
        if (!activeTagId || !annotationBackendRef || !name.trim()) return
        var result = parseData(annotationBackendRef.rename_tag_json(projectId, activeTagId, name.trim()))
        if (result !== null) refresh()
    }

    function deleteActiveTag() {
        if (!activeTagId || !annotationBackendRef) return
        var result = parseData(annotationBackendRef.delete_tag_json(projectId, activeTagId))
        if (result !== null) {
            activeTagId = ""
            refresh()
        }
    }

    function mergeSelectedTags(targetId) {
        if (!annotationBackendRef || draftTagIds.length < 2 || !targetId) return
        var sourceId = ""
        for (var i = 0; i < draftTagIds.length; ++i) {
            if (String(draftTagIds[i]) !== String(targetId)) { sourceId = String(draftTagIds[i]); break }
        }
        if (!sourceId) return
        var result = parseData(annotationBackendRef.merge_tags_json(projectId, sourceId, String(targetId)))
        if (result !== null) refresh()
    }

    function linkTargetLabel(link) {
        if (!link || !link.target) return qsTr("引用")
        if (link.target.externalUrl) return String(link.target.externalUrl)
        var ref = link.target.objectRef || ({})
        return ref.objectId || ref.kind || qsTr("引用")
    }

    function openLink(link) {
        if (!link || !link.target) return
        var resolution = annotationBackendRef
            ? parseData(annotationBackendRef.resolve_link_json(JSON.stringify(link.target))) : null
        if (!resolution) return
        if (resolution.state === "externalUrl" && link.target.externalUrl) {
            Qt.openUrlExternally(String(link.target.externalUrl))
        } else if (resolution.state === "exists" && resolution.target && resolution.target.objectRef) {
            targetActivated(resolution.target)
        } else if (resolution.state === "projectNotDownloaded") {
            errorText = qsTr("目标作品尚未下载")
        } else {
            errorText = qsTr("链接目标不存在")
        }
    }

    onTargetRefChanged: Qt.callLater(refresh)
    onProjectIdChanged: Qt.callLater(refresh)
    onChapterIdChanged: {
        selectionAnchorKey = ""; selectionAnchorRef = null
        Qt.callLater(refresh)
    }
    onSelectionStartByteChanged: {
        selectionAnchorKey = ""; selectionAnchorRef = null
        Qt.callLater(refresh)
    }
    onSelectionEndByteChanged: {
        selectionAnchorKey = ""; selectionAnchorRef = null
        Qt.callLater(refresh)
    }
    onTextRevisionChanged: {
        selectionAnchorKey = ""; selectionAnchorRef = null
        Qt.callLater(refresh)
    }
    Component.onCompleted: refresh()

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Kirigami.Units.smallSpacing
        spacing: Kirigami.Units.smallSpacing

        RowLayout {
            Layout.fillWidth: true
            Label {
                Layout.fillWidth: true
                text: root.targetLabel || qsTr("标签与引用")
                font.bold: true
                elide: Text.ElideRight
            }
            ToolButton {
                icon.name: "view-refresh"
                display: AbstractButton.IconOnly
                enabled: !root.busy
                onClicked: root.refresh()
                ToolTip.text: qsTr("刷新")
                ToolTip.visible: hovered
            }
        }

        TextField {
            id: tagSearch
            Layout.fillWidth: true
            placeholderText: qsTr("搜索标签")
            clearButtonEnabled: true
        }

        ScrollView {
            Layout.fillWidth: true
            Layout.preferredHeight: Math.min(tagList.contentHeight, Kirigami.Units.gridUnit * 5)
            Layout.minimumHeight: Math.min(tagList.contentHeight, Kirigami.Units.gridUnit * 2)
            clip: true
            ListView {
                id: tagList
                model: root.tags.filter(function(tag) {
                    return !tagSearch.text || String(tag.name || "").toLocaleLowerCase()
                        .indexOf(tagSearch.text.trim().toLocaleLowerCase()) >= 0
                })
                spacing: 1
                delegate: RowLayout {
                    required property var modelData
                    width: tagList.width
                    CheckBox {
                        Layout.fillWidth: true
                        text: modelData.name
                        checked: root.containsId(root.draftTagIds, String(modelData.tagId))
                        onToggled: root.toggleDraftTag(String(modelData.tagId), checked)
                        onPressed: root.activeTagId = String(modelData.tagId)
                    }
                    ToolButton {
                        icon.name: "overflow-menu"
                        display: AbstractButton.IconOnly
                        onClicked: {
                            root.activeTagId = String(modelData.tagId)
                            tagActions.popup()
                        }
                        Menu {
                            id: tagActions
                            MenuItem { text: qsTr("重命名"); onTriggered: renameDialog.open() }
                            MenuItem { text: qsTr("删除"); onTriggered: deleteDialog.open() }
                        }
                    }
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            TextField {
                id: newTagField
                Layout.fillWidth: true
                placeholderText: qsTr("新建标签")
                onAccepted: root.createTag()
            }
            Button { text: qsTr("添加"); enabled: newTagField.text.trim().length > 0; onClicked: root.createTag() }
        }

        RowLayout {
            Layout.fillWidth: true
            Button {
                Layout.fillWidth: true
                text: qsTr("应用标签")
                enabled: root.targetRef !== null && !root.busy
                highlighted: true
                onClicked: root.applyTags()
            }
            Button {
                text: qsTr("合并…")
                enabled: root.draftTagIds.length > 1
                onClicked: mergeDialog.open()
            }
        }

        RowLayout {
            Layout.fillWidth: true
            Label { text: qsTr("链接"); font.bold: true; Layout.fillWidth: true }
            ToolButton {
                icon.name: "list-add"
                display: AbstractButton.IconOnly
                enabled: root.targetRef !== null
                onClicked: root.openLinkPicker(null)
                ToolTip.text: qsTr("添加超链接")
                ToolTip.visible: hovered
            }
        }
        ListView {
            Layout.fillWidth: true
            Layout.preferredHeight: Math.min(contentHeight, Kirigami.Units.gridUnit * 5)
            clip: true
            model: root.outgoingLinks
            delegate: RowLayout {
                required property var modelData
                width: ListView.view.width
                ItemDelegate {
                    Layout.fillWidth: true
                    text: root.linkTargetLabel(modelData)
                    icon.name: "go-jump"
                    onClicked: root.openLink(modelData)
                }
                ToolButton {
                    icon.name: "overflow-menu"
                    display: AbstractButton.IconOnly
                    onClicked: linkActions.popup()
                }
                Menu {
                    id: linkActions
                    MenuItem { text: qsTr("修改目标"); onTriggered: root.openLinkPicker(modelData) }
                    MenuItem { text: qsTr("删除链接"); onTriggered: root.deleteLink(modelData) }
                }
            }
        }

        Label {
            Layout.fillWidth: true
            visible: root.outgoingLinks.length === 0
            text: qsTr("暂无链接")
            color: Kirigami.Theme.disabledTextColor
        }

        Label { text: qsTr("反向引用"); font.bold: true }
        ListView {
            Layout.fillWidth: true
            Layout.preferredHeight: Math.min(contentHeight, Kirigami.Units.gridUnit * 5)
            clip: true
            model: root.backlinks
            delegate: ItemDelegate {
                required property var modelData
                width: ListView.view.width
                text: modelData.source ? modelData.source.objectId : qsTr("来源")
                icon.name: "go-jump"
                onClicked: root.targetActivated({ objectRef: modelData.source, externalUrl: null })
            }
        }

        Label {
            Layout.fillWidth: true
            visible: root.backlinks.length === 0
            text: qsTr("暂无反向引用")
            color: Kirigami.Theme.disabledTextColor
        }

        RowLayout {
            Layout.fillWidth: true
            Label { text: qsTr("登记名称"); font.bold: true; Layout.fillWidth: true }
            ToolButton {
                icon.name: "list-add"
                display: AbstractButton.IconOnly
                onClicked: {
                    root.activeEntryId = ""
                    entryNameField.text = ""
                    entryAliasesField.text = ""
                    entryMisspellingsField.text = ""
                    entryAutoLink.checked = true
                    entryDialog.open()
                }
                ToolTip.text: qsTr("登记名称、别名和自动链接")
                ToolTip.visible: hovered
            }
        }
        ListView {
            Layout.fillWidth: true
            Layout.preferredHeight: Math.min(contentHeight, Kirigami.Units.gridUnit * 4)
            clip: true
            model: root.namedEntries
            delegate: RowLayout {
                required property var modelData
                width: ListView.view.width
                Label {
                    Layout.fillWidth: true
                    text: String(modelData.canonicalName || "")
                    elide: Text.ElideRight
                }
                Label {
                    text: modelData.autoLinkEnabled ? qsTr("自动") : qsTr("手动")
                    color: Kirigami.Theme.disabledTextColor
                }
                ToolButton {
                    icon.name: "document-edit"
                    display: AbstractButton.IconOnly
                    onClicked: root.editEntry(modelData)
                    ToolTip.text: qsTr("编辑名称")
                    ToolTip.visible: hovered
                }
                ToolButton {
                    icon.name: "edit-delete"
                    display: AbstractButton.IconOnly
                    onClicked: root.deleteEntry(modelData)
                    ToolTip.text: qsTr("删除名称")
                    ToolTip.visible: hovered
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
    }

    Dialog {
        id: renameDialog
        title: qsTr("重命名标签")
        modal: true
        standardButtons: Dialog.Ok | Dialog.Cancel
        onAccepted: root.renameActiveTag(renameField.text)
        contentItem: TextField { id: renameField; text: ""; onVisibleChanged: if (visible) text = "" }
    }
    Dialog {
        id: deleteDialog
        title: qsTr("删除标签")
        modal: true
        standardButtons: Dialog.Ok | Dialog.Cancel
        Label { text: qsTr("删除此标签及其所有绑定？") }
        onAccepted: root.deleteActiveTag()
    }
    Dialog {
        id: mergeDialog
        title: qsTr("合并标签")
        modal: true
        standardButtons: Dialog.Ok | Dialog.Cancel
        property string targetTagId: ""
        ColumnLayout {
            Label { text: qsTr("已选中的第一个标签会合并到目标标签。") }
            ComboBox {
                Layout.fillWidth: true
                model: root.tags.filter(function(tag) { return !root.containsId(root.draftTagIds.slice(1), String(tag.tagId)) })
                textRole: "name"
                onCurrentIndexChanged: if (currentIndex >= 0 && model[currentIndex]) mergeDialog.targetTagId = String(model[currentIndex].tagId)
            }
        }
        onAccepted: root.mergeSelectedTags(targetTagId)
    }
    Dialog {
        id: entryDialog
        title: root.activeEntryId ? qsTr("编辑登记名称") : qsTr("登记名称")
        modal: true
        standardButtons: Dialog.Save | Dialog.Cancel
        onAccepted: root.saveEntry()
        contentItem: ColumnLayout {
            TextField { id: entryNameField; Layout.fillWidth: true; placeholderText: qsTr("标准名称") }
            TextField { id: entryAliasesField; Layout.fillWidth: true; placeholderText: qsTr("别名，以逗号分隔") }
            TextField { id: entryMisspellingsField; Layout.fillWidth: true; placeholderText: qsTr("确认的误写，以逗号分隔") }
            CheckBox { id: entryAutoLink; text: qsTr("正文中自动识别并链接") }
        }
    }
}
