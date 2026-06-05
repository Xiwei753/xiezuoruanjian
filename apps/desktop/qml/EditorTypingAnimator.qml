// =============================================================================
// EditorTypingAnimator.qml — 编辑器文字吐字/吞入动画
// =============================================================================

import QtQuick

Item {
    id: root

    property var targetTextArea: null
    property var documentHandler: null
    property var overlayItem: parent
    property var dt: null
    property bool animationEnabled: true
    property bool suppressed: false
    property int animationDuration: 160
    property int maxAnimatedChars: 8

    property string previousText: ""
    property int nextAnimationId: 1

    anchors.fill: parent
    z: 2
    visible: animationEnabled && !suppressed && targetTextArea && documentHandler

    ListModel { id: typingAnimations }

    function readPlainText() {
        if (!targetTextArea) return "";
        try {
            var len = targetTextArea.length || 0;
            if (len > 0 && targetTextArea.getText) {
                return String(targetTextArea.getText(0, len)).replace(/\u2029/g, "\n");
            }
        } catch (e) {
        }
        return targetTextArea.text ? String(targetTextArea.text).replace(/\u2029/g, "\n") : "";
    }

    function commonPrefixLength(oldText, newText) {
        var limit = Math.min(oldText.length, newText.length);
        var i = 0;
        while (i < limit && oldText.charAt(i) === newText.charAt(i)) i++;
        return i;
    }

    function commonSuffixLength(oldText, newText, prefixLength) {
        var oldIndex = oldText.length - 1;
        var newIndex = newText.length - 1;
        var count = 0;
        while (oldIndex >= prefixLength && newIndex >= prefixLength && oldText.charAt(oldIndex) === newText.charAt(newIndex)) {
            oldIndex--;
            newIndex--;
            count++;
        }
        return count;
    }

    function clearHiddenRanges() {
        if (documentHandler && documentHandler.clear_hidden_text_ranges) {
            documentHandler.clear_hidden_text_ranges();
        }
    }

    function resetTextSnapshot() {
        clearHiddenRanges();
        typingAnimations.clear();
        previousText = readPlainText();
    }

    function shouldAnimateText(text) {
        return text.length > 0 && text.length <= maxAnimatedChars && text.indexOf("\n") < 0;
    }

    function rectForPosition(position) {
        if (!targetTextArea || !targetTextArea.positionToRectangle) {
            return { "x": 0, "y": 0, "height": targetTextArea ? Math.max(targetTextArea.font.pixelSize * 1.2, 18) : 18 };
        }

        var safePos = Math.max(0, Math.min(position, targetTextArea.length || 0));
        var rect = targetTextArea.positionToRectangle(safePos);
        var mapped = targetTextArea.mapToItem(overlayItem || parent, rect.x || 0, rect.y || 0);
        return {
            "x": mapped ? mapped.x : 0,
            "y": mapped ? mapped.y : 0,
            "height": rect && rect.height ? rect.height : Math.max(targetTextArea.font.pixelSize * 1.2, 18)
        };
    }

    function appendAnimation(kind, startIndex, text, visualWidth) {
        var rect = rectForPosition(startIndex);
        var id = nextAnimationId++;
        typingAnimations.append({
            "animationId": id,
            "kind": kind,
            "startIndex": startIndex,
            "textLength": text.length,
            "text": text,
            "x": rect.x,
            "y": rect.y,
            "height": rect.height,
            "visualWidth": visualWidth || 0
        });
    }

    function animateInsert(startIndex, text) {
        var startRect = rectForPosition(startIndex);
        var endRect = rectForPosition(startIndex + text.length);
        var sameLine = Math.abs((endRect.y || 0) - (startRect.y || 0)) < 2;
        var visualWidth = sameLine ? Math.max(1, (endRect.x || 0) - (startRect.x || 0)) : 0;
        if (documentHandler && documentHandler.hide_text_range) {
            documentHandler.hide_text_range(startIndex, text.length);
        }
        appendAnimation("insert", startIndex, text, visualWidth);
    }

    function animateDelete(startIndex, text) {
        appendAnimation("delete", startIndex, text, 0);
    }

    function finishAnimation(animationId) {
        for (var i = 0; i < typingAnimations.count; i++) {
            var item = typingAnimations.get(i);
            if (item.animationId === animationId) {
                if (item.kind === "insert" && documentHandler && documentHandler.show_text_range) {
                    documentHandler.show_text_range(item.startIndex, item.textLength);
                }
                typingAnimations.remove(i);
                return;
            }
        }
    }

    function handleTextChanged(oldText, newText) {
        if (!animationEnabled || suppressed || !documentHandler || !targetTextArea) {
            resetTextSnapshot();
            return;
        }

        var prefix = commonPrefixLength(oldText, newText);
        var suffix = commonSuffixLength(oldText, newText, prefix);
        var removedLength = oldText.length - prefix - suffix;
        var insertedLength = newText.length - prefix - suffix;

        if (insertedLength > 0 && removedLength === 0) {
            var inserted = newText.substr(prefix, insertedLength);
            if (shouldAnimateText(inserted)) {
                animateInsert(prefix, inserted);
                return;
            }
        }

        if (removedLength > 0 && insertedLength === 0) {
            var removed = oldText.substr(prefix, removedLength);
            if (shouldAnimateText(removed)) {
                animateDelete(prefix, removed);
                return;
            }
        }

        clearHiddenRanges();
    }

    onTargetTextAreaChanged: resetTextSnapshot()
    onDocumentHandlerChanged: resetTextSnapshot()
    onSuppressedChanged: if (suppressed) resetTextSnapshot()
    onAnimationEnabledChanged: if (!animationEnabled) resetTextSnapshot()

    Connections {
        target: root.targetTextArea
        function onTextChanged() {
            var current = root.readPlainText();
            root.handleTextChanged(root.previousText, current);
            root.previousText = current;
        }
    }

    Repeater {
        model: typingAnimations

        delegate: Item {
            id: animationItem
            property real progress: model.kind === "insert" ? 0 : 1
            property real textWidth: Math.max(1, animatedText.implicitWidth)
            property real fullWidth: Math.max(1, model.visualWidth > 0 ? model.visualWidth : textWidth)

            x: model.kind === "delete" ? model.x + fullWidth * (1 - progress) : model.x
            y: model.y
            width: model.kind === "delete" ? Math.max(1, fullWidth * progress) : fullWidth
            height: model.height
            clip: model.kind === "delete"
            opacity: model.kind === "delete" ? progress : 1

            Rectangle {
                visible: model.kind === "insert"
                x: 0
                y: 0
                width: animationItem.fullWidth
                height: animationItem.height
                color: root.dt ? root.dt.editorBackground : "#191C21"
                z: 0
            }

            Item {
                id: revealClip
                x: 0
                y: 0
                width: Math.max(1, animationItem.fullWidth * animationItem.progress)
                height: animationItem.height
                clip: true
                z: 1

                Text {
                    id: animatedText
                    text: model.text
                    color: root.dt ? root.dt.editorText : "#E2E2E5"
                    font.pixelSize: root.targetTextArea ? root.targetTextArea.font.pixelSize : 16
                    font.family: root.targetTextArea ? root.targetTextArea.font.family : "serif"
                    y: Math.max(0, (animationItem.height - implicitHeight) / 2)
                }
            }

            NumberAnimation on progress {
                from: model.kind === "insert" ? 0 : 1
                to: model.kind === "insert" ? 1 : 0
                duration: Math.max(0, root.animationDuration)
                easing.type: model.kind === "insert" ? Easing.OutCubic : Easing.InCubic
                onStopped: root.finishAnimation(model.animationId)
            }
        }
    }
}
