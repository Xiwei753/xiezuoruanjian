// =============================================================================
// CorrectionPanel.qml — 写作工具侧栏中的校对内容
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Item {
    id: root

    // 校对状态只由 WritingWorkspace 的 EditorController 持有。
    property var editorControllerRef: null
    property bool resourcesExpanded: false
    // Keep the resources pane independently scrollable instead of letting a
    // long registered-name list push the suggestion list out of the sidebar.
    property real correctionResourcesMaxHeight: Math.max(
            Kirigami.Units.gridUnit * 6,
            Math.min(Kirigami.Units.gridUnit * 22, root.height * 0.5))

    onVisibleChanged: {
        if (visible && root.editorControllerRef) {
            Qt.callLater(function() {
                if (!root.visible || !root.editorControllerRef) return;
                root.editorControllerRef.refreshCorrectionDictionaryStatus();
                root.editorControllerRef.refreshCorrectionEntityNames();
            });
        }
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Kirigami.Units.smallSpacing
        spacing: Kirigami.Units.smallSpacing

        Label {
            Layout.fillWidth: true
            text: qsTr("校对 · %1 条建议").arg(
                      root.editorControllerRef
                      ? root.editorControllerRef.correctionSuggestions.length : 0)
            font.bold: true
            elide: Text.ElideRight
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing

            Button {
                Layout.fillWidth: true
                text: root.editorControllerRef && root.editorControllerRef.correctionCheckPending
                      ? qsTr("正在检查") : qsTr("检查本章")
                enabled: !!(root.editorControllerRef
                            && root.editorControllerRef.chapterId
                            && !root.editorControllerRef.correctionCheckPending)
                onClicked: root.editorControllerRef.requestCorrectionChapter()
            }

            Button {
                Layout.fillWidth: true
                text: qsTr("清除标注")
                enabled: !!(root.editorControllerRef
                            && root.editorControllerRef.correctionSuggestions.length)
                onClicked: root.editorControllerRef.clearCorrectionAnnotations()
            }
        }

        Label {
            Layout.fillWidth: true
            visible: !!(root.editorControllerRef
                        && (root.editorControllerRef.correctionError
                            || root.editorControllerRef.correctionStatus))
            text: root.editorControllerRef
                  ? (root.editorControllerRef.correctionError
                     || root.editorControllerRef.correctionStatus) : ""
            color: root.editorControllerRef && root.editorControllerRef.correctionError
                   ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.textColor
            wrapMode: Text.Wrap
        }

        ListView {
            id: correctionSuggestionList
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            spacing: Kirigami.Units.smallSpacing
            model: root.editorControllerRef
                   ? root.editorControllerRef.correctionSuggestions : []

            delegate: Rectangle {
                id: suggestionItem
                required property var modelData
                width: correctionSuggestionList.width
                height: suggestionContent.implicitHeight + Kirigami.Units.largeSpacing
                radius: Kirigami.Units.cornerRadius
                color: Kirigami.Theme.alternateBackgroundColor
                // Uncertain checks must read as hints, not as red errors.
                opacity: String(modelData.severity || "").toLowerCase() === "error" ? 1.0 : 0.86

                ColumnLayout {
                    id: suggestionContent
                    anchors.fill: parent
                    anchors.margins: Kirigami.Units.smallSpacing
                    spacing: Kirigami.Units.smallSpacing / 2

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: Kirigami.Units.smallSpacing / 2

                        // Clicking only the source word jumps to and selects
                        // the exact reported range in the editor.
                        Label {
                            id: correctionOriginal
                            Layout.fillWidth: true
                            Layout.minimumWidth: 0
                            text: String(modelData.original || "")
                            wrapMode: Text.Wrap

                            MouseArea {
                                anchors.fill: parent
                                cursorShape: Qt.PointingHandCursor
                                onClicked: {
                                    if (root.editorControllerRef)
                                        root.editorControllerRef.jumpToCorrection(modelData)
                                }
                            }
                        }

                        Label {
                            text: "→"
                        }

                        Label {
                            Layout.fillWidth: true
                            Layout.minimumWidth: 0
                            text: String(modelData.replacement || "")
                            wrapMode: Text.Wrap
                        }
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: Kirigami.Units.smallSpacing

                        Button {
                            text: qsTr("替换")
                            highlighted: true
                            onClicked: root.editorControllerRef.acceptCorrection(modelData)
                        }

                        Item { Layout.fillWidth: true }

                        ToolButton {
                            id: overflowButton
                            text: qsTr("⋯")
                            Accessible.name: qsTr("更多校对操作")
                            onClicked: suggestionMenu.popup()
                        }

                        Menu {
                            id: suggestionMenu

                            MenuItem {
                                text: qsTr("忽略一次")
                                onTriggered: root.editorControllerRef.ignoreCorrection(modelData, "once")
                            }
                            MenuItem {
                                text: qsTr("忽略本章")
                                onTriggered: root.editorControllerRef.ignoreCorrection(modelData, "chapter")
                            }
                            MenuItem {
                                text: qsTr("加入作品词典…")
                                onTriggered: {
                                    dictionaryEntryDialog.suggestion = modelData;
                                    dictionaryEntryDialog.open();
                                }
                            }
                            MenuItem {
                                text: qsTr("登记名称对应…")
                                onTriggered: {
                                    entityNameDialog.suggestion = modelData;
                                    entityNameDialog.open();
                                }
                            }
                            MenuSeparator {}
                            MenuItem {
                                text: qsTr("停用此规则")
                                onTriggered: root.editorControllerRef.ignoreCorrection(modelData, "rule")
                            }
                        }
                    }
                }
            }
        }

        Button {
            Layout.fillWidth: true
            text: root.resourcesExpanded ? qsTr("收起校对资源") : qsTr("校对资源")
            icon.name: root.resourcesExpanded ? "go-up" : "go-down"
            onClicked: {
                root.resourcesExpanded = !root.resourcesExpanded;
                if (root.resourcesExpanded && root.editorControllerRef)
                    root.editorControllerRef.refreshCorrectionEntityNames();
            }
        }

        ScrollView {
            id: correctionResourcesScroll
            Layout.fillWidth: true
            Layout.preferredHeight: Math.min(correctionResourcesContent.implicitHeight,
                                             root.correctionResourcesMaxHeight)
            Layout.maximumHeight: root.correctionResourcesMaxHeight
            visible: root.resourcesExpanded
            clip: true
            ScrollBar.vertical.policy: ScrollBar.AsNeeded

            ColumnLayout {
                id: correctionResourcesContent
                width: correctionResourcesScroll.availableWidth
                spacing: Kirigami.Units.smallSpacing

            TextField {
                id: correctionModelPathField
                Layout.fillWidth: true
                placeholderText: qsTr("本地模型资源目录（含 model.onnx、vocab.txt 与 tokenizer.json）")
                selectByMouse: true
                onAccepted: {
                    if (root.editorControllerRef)
                        root.editorControllerRef.setCorrectionModelPath(text);
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing

                Button {
                    Layout.fillWidth: true
                    text: qsTr("选择模型")
                    onClicked: {
                        var selectedPath = root.editorControllerRef
                                ? root.editorControllerRef.chooseCorrectionModelPath() : "";
                        if (selectedPath) correctionModelPathField.text = selectedPath;
                    }
                }
                Button {
                    Layout.fillWidth: true
                    text: qsTr("设置路径")
                    onClicked: {
                        if (root.editorControllerRef)
                            root.editorControllerRef.setCorrectionModelPath(correctionModelPathField.text);
                    }
                }
            }

            Button {
                Layout.fillWidth: true
                text: qsTr("导入词库文件")
                onClicked: {
                    if (root.editorControllerRef)
                        root.editorControllerRef.importCorrectionDictionary();
                }
            }

            Label {
                Layout.fillWidth: true
                text: qsTr("固定命名的 .txt")
                color: Kirigami.Theme.disabledTextColor
                elide: Text.ElideRight
            }

            Flow {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing

                Repeater {
                    model: root.editorControllerRef
                           ? root.editorControllerRef.correctionDictionaryResources : []
                    delegate: Label {
                        required property var modelData
                        text: qsTr("%1 · %2")
                              .arg(modelData.name)
                              .arg(modelData.ready ? qsTr("已准备") : qsTr("待准备"))
                        color: modelData.ready ? Kirigami.Theme.positiveTextColor
                                               : Kirigami.Theme.disabledTextColor
                        font.pixelSize: Kirigami.Units.gridUnit * 0.75
                    }
                }
            }

            Label {
                Layout.fillWidth: true
                visible: !!(root.editorControllerRef
                            && root.editorControllerRef.correctionDictionaryError)
                text: root.editorControllerRef
                      ? root.editorControllerRef.correctionDictionaryError : ""
                color: Kirigami.Theme.negativeTextColor
                wrapMode: Text.Wrap
            }

            Label {
                Layout.fillWidth: true
                visible: !!(root.editorControllerRef
                            && root.editorControllerRef.correctionEntityNames.length > 0)
                text: qsTr("名称对应关系")
                font.bold: true
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 0
                visible: !!(root.editorControllerRef
                            && root.editorControllerRef.correctionEntityNames.length > 0)

                Repeater {
                    model: root.editorControllerRef
                           ? root.editorControllerRef.correctionEntityNames : []
                    delegate: RowLayout {
                        required property var modelData
                        Layout.fillWidth: true
                        spacing: Kirigami.Units.smallSpacing

                        Label {
                            Layout.fillWidth: true
                            text: qsTr("%1 · %2 · %3")
                                  .arg(modelData.canonical)
                                  .arg(modelData.name)
                                  .arg(modelData.kind === "misspelling"
                                       ? qsTr("错误写法") : qsTr("别名"))
                            wrapMode: Text.Wrap
                        }

                        ToolButton {
                            text: qsTr("删除")
                            onClicked: {
                                if (root.editorControllerRef)
                                    root.editorControllerRef.removeCorrectionEntityName(
                                                modelData.canonical, modelData.name);
                            }
                        }
                    }
                }
            }
            }
        }
    }

    // The engine's suggested span may be only one character, while a work
    // dictionary entry must be the author-confirmed complete name or term.
    Dialog {
        id: dictionaryEntryDialog
        property var suggestion: null
        title: qsTr("加入作品词典")
        modal: true
        focus: true
        width: Math.min(root.width - Kirigami.Units.largeSpacing * 2,
                        Kirigami.Units.gridUnit * 26)
        standardButtons: Dialog.Ok | Dialog.Cancel
        onOpened: {
            var draft = root.editorControllerRef
                    ? root.editorControllerRef.correctionDictionaryEntryDraft(suggestion) : "";
            dictionaryEntryField.text = draft;
            dictionaryKindBox.currentIndex = 0;
            dictionaryEntryField.forceActiveFocus();
            dictionaryEntryField.selectAll();
        }
        onAccepted: {
            if (root.editorControllerRef) {
                root.editorControllerRef.addCorrectionWord(
                    dictionaryEntryField.text,
                    dictionaryKindBox.currentIndex === 1 ? "person"
                    : dictionaryKindBox.currentIndex === 2 ? "place" : "term");
            }
        }

        contentItem: ColumnLayout {
            spacing: Kirigami.Units.smallSpacing

            Label {
                Layout.fillWidth: true
                text: qsTr("完整词条（可修改为作品中实际使用的完整名称）")
                wrapMode: Text.Wrap
            }

            TextField {
                id: dictionaryEntryField
                Layout.fillWidth: true
                selectByMouse: true
                placeholderText: qsTr("输入完整词条")
            }

            Label {
                Layout.fillWidth: true
                text: qsTr("类型")
            }

            ComboBox {
                id: dictionaryKindBox
                Layout.fillWidth: true
                model: [qsTr("普通术语"), qsTr("人物"), qsTr("地点")]
                currentIndex: 0
            }
        }
    }

    // Only explicit author relations are allowed. There is no automatic
    // similarity lookup for names that were never registered.
    Dialog {
        id: entityNameDialog
        property var suggestion: null
        title: qsTr("登记名称对应")
        modal: true
        focus: true
        width: Math.min(root.width - Kirigami.Units.largeSpacing * 2,
                        Kirigami.Units.gridUnit * 27)
        standardButtons: Dialog.Ok | Dialog.Cancel
        onOpened: {
            entityCanonicalField.text = "";
            entityWrittenField.text = root.editorControllerRef
                    ? root.editorControllerRef.correctionEntityWrittenDraft(suggestion) : "";
            entityRelationKindBox.currentIndex = 0;
            entityCanonicalField.forceActiveFocus();
        }
        onAccepted: {
            if (root.editorControllerRef) {
                root.editorControllerRef.addCorrectionEntityName(
                    entityCanonicalField.text,
                    entityWrittenField.text,
                    entityRelationKindBox.currentIndex === 1 ? "alias" : "misspelling");
            }
        }

        contentItem: ColumnLayout {
            spacing: Kirigami.Units.smallSpacing

            Label {
                Layout.fillWidth: true
                text: qsTr("完整原名必须是作品词典中的人物或地点；此操作不会自动识别未登记的名字。")
                wrapMode: Text.Wrap
            }

            TextField {
                id: entityCanonicalField
                Layout.fillWidth: true
                selectByMouse: true
                placeholderText: qsTr("输入已登记的正式名称")
            }

            TextField {
                id: entityWrittenField
                Layout.fillWidth: true
                selectByMouse: true
                placeholderText: qsTr("输入别名或已确认的错误写法")
            }

            Label {
                Layout.fillWidth: true
                text: qsTr("对应关系")
            }

            ComboBox {
                id: entityRelationKindBox
                Layout.fillWidth: true
                model: [qsTr("错误写法（建议替换为原名）"), qsTr("别名（保护，不提示）")]
                currentIndex: 0
            }
        }
    }
}
