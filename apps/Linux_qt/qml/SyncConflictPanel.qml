// =============================================================================
// SyncConflictPanel.qml — 同步冲突解决面板
// =============================================================================
//
// 层级：Linux_qt UI 层（QML UI 组件）
// 职责：展示本地/远端冲突正文并排只读预览，提供三个解决动作。
// 约束：
//   - 纯 UI 组件，所有数据通过 SyncBackend QML 方法调用 Core API 拿。
//   - 不自己拼磁盘路径读 conflicts.json。
//   - 关闭只收起侧栏（发 closeRequested），冲突仍保留。
//   - 窄宽度（width < 500）改用"本地/远端"页签，不缩成看不清的两条窄栏。
//   - RemoteDeleted 时远端区域显示"远端已删除此文件"，不伪造空字符串。
//   - 用 DesignTokens 统一样式。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    required property var dt
    // SyncBackend 引用，用于调用 list_sync_conflicts / load_sync_conflict_preview / resolve_*。
    property var syncBackendRef: null
    // 当前项目 ID（由 WritingWorkspace.workspaceProjectId 传入）。
    property string projectId: ""
    // 当前选中的冲突路径（由外部指定，或内部自动选第一个）。
    property string conflictPath: ""

    // 关闭只收起侧栏，冲突仍保留。
    signal closeRequested()
    // 冲突被解决后发出，通知外部刷新（RightDrawer/WritingWorkspace 可据此重载列表）。
    signal conflictsResolved()

    // ── DesignTokens（参考 RightDrawer.qml） ──
    readonly property color _sidebar: dt.sidebar
    readonly property color _border: dt.border
    readonly property color _accentSoft: dt.accentSoft
    readonly property color _accentText: dt.accentText
    readonly property color _card: dt.card
    readonly property color _surface: dt.surface
    readonly property color _surfaceVariant: dt.surfaceVariant
    readonly property color _textPrimary: dt.textPrimary
    readonly property color _textSecondary: dt.textSecondary
    readonly property color _textMuted: dt.textMuted
    readonly property color _primary: dt.primary
    readonly property color _error: dt.error
    readonly property int _sp4: dt.sp4
    readonly property int _sp8: dt.sp8
    readonly property int _sp12: dt.sp12
    readonly property int _sp16: dt.sp16
    readonly property int _sp24: dt.sp24
    readonly property int _radiusSm: dt.radiusSm
    readonly property int _radiusMd: dt.radiusMd
    readonly property real _fontSm: dt.fontSmPt
    readonly property real _fontMd: dt.fontMdPt
    readonly property real _fontLg: dt.fontLgPt

    // 窄宽度阈值：低于此值改用页签而非并排。
    readonly property real _narrowThreshold: 500
    readonly property bool _isNarrow: root.width < _narrowThreshold

    // ── 内部状态 ──
    // list_sync_conflicts 返回的冲突数组（SyncConflictDto[]，camelCase 字段）。
    property var conflicts: []
    // load_sync_conflict_preview 返回的预览对象（{ path, kind, createdAt, localContent, remoteContent, remoteDeleted }）。
    property var preview: null
    // 当前冲突在 conflicts 数组中的索引。
    property int currentIndex: 0
    property bool loading: false
    // 窄宽页签：0=本地，1=远端。
    property int narrowTab: 0

    color: "transparent"

    // ── 数据加载 ──

    function reloadConflicts() {
        if (!root.syncBackendRef || !root.projectId) {
            root.conflicts = [];
            root.preview = null;
            return;
        }
        root.loading = true;
        var raw = root.syncBackendRef.list_sync_conflicts(root.projectId);
        root.loading = false;
        var resp;
        try { resp = JSON.parse(raw); } catch (e) { resp = null; }
        if (resp && resp.success && resp.data) {
            root.conflicts = resp.data.conflicts || [];
            // 选中 conflictPath 对应的冲突，否则选第一个。
            var found = -1;
            if (root.conflictPath) {
                for (var i = 0; i < root.conflicts.length; i++) {
                    if (root.conflicts[i].localPath === root.conflictPath) {
                        found = i;
                        break;
                    }
                }
            }
            if (found >= 0) {
                root.currentIndex = found;
            } else if (root.conflicts.length > 0) {
                root.currentIndex = 0;
                root.conflictPath = root.conflicts[0].localPath;
            } else {
                root.currentIndex = 0;
                root.conflictPath = "";
            }
            loadPreview();
        } else {
            root.conflicts = [];
            root.preview = null;
        }
    }

    function loadPreview() {
        if (!root.syncBackendRef || !root.projectId || !root.conflictPath) {
            root.preview = null;
            return;
        }
        var raw = root.syncBackendRef.load_sync_conflict_preview(root.projectId, root.conflictPath);
        var resp;
        try { resp = JSON.parse(raw); } catch (e) { resp = null; }
        if (resp && resp.success && resp.data) {
            root.preview = resp.data.preview || null;
        } else {
            root.preview = null;
        }
    }

    function resolveAction(action) {
        if (!root.syncBackendRef || !root.projectId || !root.conflictPath) return;
        var raw;
        if (action === "keep_local") {
            raw = root.syncBackendRef.resolve_conflict_keep_local(root.projectId, root.conflictPath);
        } else if (action === "take_remote") {
            raw = root.syncBackendRef.resolve_conflict_take_remote(root.projectId, root.conflictPath);
        } else if (action === "mark_merged") {
            raw = root.syncBackendRef.resolve_conflict_mark_merged(root.projectId, root.conflictPath);
        } else {
            return;
        }
        var resp;
        try { resp = JSON.parse(raw); } catch (e) { resp = null; }
        if (resp && resp.success) {
            root.conflictsResolved();
            // 解决一个冲突后刷新列表，自动跳到下一个或清空。
            var resolvedPath = root.conflictPath;
            root.conflictPath = "";
            reloadConflicts();
            // 若仍有冲突且未自动选中，选第一个。
            if (root.conflicts.length > 0 && !root.conflictPath) {
                root.conflictPath = root.conflicts[0].localPath;
                root.currentIndex = 0;
                loadPreview();
            }
        }
    }

    function selectIndex(idx) {
        if (idx < 0 || idx >= root.conflicts.length) return;
        root.currentIndex = idx;
        root.conflictPath = root.conflicts[idx].localPath;
        loadPreview();
    }

    onConflictPathChanged: loadPreview()
    Component.onCompleted: reloadConflicts()

    // ── 布局 ──

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // ── Header：文件名 + 第几个/共几个 + 关闭 ──
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 48
            color: "transparent"

            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: root._sp12
                anchors.rightMargin: root._sp8
                spacing: root._sp8

                AppText {
                    dt: root.dt
                    text: root.conflictPath ? root.conflictPath : qsTr("无冲突")
                    color: root._textPrimary
                    font.pointSize: root._fontMd
                    font.weight: Font.DemiBold
                    Layout.fillWidth: true
                    elide: Text.ElideMiddle
                }

                AppText {
                    dt: root.dt
                    visible: root.conflicts.length > 0
                    text: (root.currentIndex + 1) + " / " + root.conflicts.length
                    color: root._textSecondary
                    font.pointSize: root._fontSm
                }

                // 关闭按钮 — 只收起侧栏，冲突仍保留。
                Rectangle {
                    width: 24; height: 24
                    radius: 12
                    color: closeHover.containsMouse ? root._card : "transparent"

                    AppText {
                        dt: root.dt
                        anchors.centerIn: parent
                        text: "\u2715"
                        color: root._textMuted
                        font.pointSize: root._fontSm
                    }

                    MouseArea {
                        id: closeHover
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.closeRequested()
                    }
                }
            }
        }

        Rectangle { Layout.fillWidth: true; height: 1; color: root._border }

        // ── Content：本地/远端预览 ──
        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true

            // 空状态：无冲突。
            ColumnLayout {
                anchors.centerIn: parent
                spacing: root._sp16
                visible: root.conflicts.length === 0 && !root.loading

                AppText {
                    text: "\u2713"
                    dt: root.dt
                    font.pointSize: root._fontLg
                    color: root._textSecondary
                    Layout.alignment: Qt.AlignHCenter
                }
                AppText {
                    text: qsTr("没有未解决的冲突")
                    dt: root.dt
                    color: root._textPrimary
                    font.pointSize: root._fontMd
                    font.weight: Font.DemiBold
                    Layout.alignment: Qt.AlignHCenter
                }
            }

            // 加载中。
            AppText {
                anchors.centerIn: parent
                visible: root.loading
                text: qsTr("加载中…")
                dt: root.dt
                color: root._textMuted
                font.pointSize: root._fontSm
            }

            // 有冲突时展示预览。
            Item {
                anchors.fill: parent
                visible: root.conflicts.length > 0 && !root.loading

                // 窄宽页签栏。
                Rectangle {
                    id: narrowTabBar
                    visible: root._isNarrow
                    anchors.top: parent.top
                    anchors.left: parent.left
                    anchors.right: parent.right
                    height: 36
                    color: "transparent"

                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: root._sp8
                        anchors.rightMargin: root._sp8
                        spacing: root._sp4

                        Repeater {
                            model: [qsTr("本地"), qsTr("远端")]

                            Rectangle {
                                Layout.fillWidth: true
                                height: 28
                                radius: root._radiusSm
                                color: root.narrowTab === index ?
                                       root._accentSoft :
                                       (narrowTabHover.containsMouse ? root._card : "transparent")

                                AppText {
                                    dt: root.dt
                                    anchors.centerIn: parent
                                    text: modelData
                                    color: root.narrowTab === index ? root._accentText : root._textSecondary
                                    font.pointSize: root._fontSm
                                    font.weight: root.narrowTab === index ? Font.DemiBold : Font.Normal
                                }

                                MouseArea {
                                    id: narrowTabHover
                                    anchors.fill: parent
                                    hoverEnabled: true
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: root.narrowTab = index
                                }
                            }
                        }
                    }
                }

                // 并排展示（宽屏）。
                RowLayout {
                    id: sideBySide
                    visible: !root._isNarrow
                    anchors.fill: parent
                    anchors.margins: root._sp8
                    spacing: root._sp8

                    // 本地正文
                    ColumnLayout {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        spacing: root._sp4

                        AppText {
                            text: qsTr("本地")
                            dt: root.dt
                            color: root._textSecondary
                            font.pointSize: root._fontSm
                            font.weight: Font.DemiBold
                        }

                        ScrollView {
                            Layout.fillWidth: true
                            Layout.fillHeight: true
                            clip: true

                            TextArea {
                                readOnly: true
                                text: root.preview ? (root.preview.localContent || "") : ""
                                color: root._textPrimary
                                font.pointSize: root._fontSm
                                wrapMode: TextArea.Wrap
                                background: Rectangle {
                                    color: root._surface
                                    border.color: root._border
                                    border.width: 1
                                    radius: root._radiusSm
                                }
                            }
                        }
                    }

                    // 远端正文 / 已删除提示
                    ColumnLayout {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        spacing: root._sp4

                        AppText {
                            text: qsTr("远端")
                            dt: root.dt
                            color: root._textSecondary
                            font.pointSize: root._fontSm
                            font.weight: Font.DemiBold
                        }

                        // RemoteDeleted：显示"远端已删除此文件"，不伪造空字符串。
                        ColumnLayout {
                            Layout.fillWidth: true
                            Layout.fillHeight: true
                            visible: root.preview && root.preview.remoteDeleted === true

                            Rectangle {
                                Layout.fillWidth: true
                                Layout.fillHeight: true
                                color: root._surface
                                border.color: root._border
                                border.width: 1
                                radius: root._radiusSm

                                AppText {
                                    anchors.centerIn: parent
                                    text: qsTr("远端已删除此文件")
                                    dt: root.dt
                                    color: root._textMuted
                                    font.pointSize: root._fontSm
                                    wrapMode: Text.Wrap
                                    horizontalAlignment: Text.AlignHCenter
                                }
                            }
                        }

                        // BothChanged：显示远端 snapshot 正文。
                        ScrollView {
                            Layout.fillWidth: true
                            Layout.fillHeight: true
                            visible: !(root.preview && root.preview.remoteDeleted === true)
                            clip: true

                            TextArea {
                                readOnly: true
                                text: root.preview ? (root.preview.remoteContent || "") : ""
                                color: root._textPrimary
                                font.pointSize: root._fontSm
                                wrapMode: TextArea.Wrap
                                background: Rectangle {
                                    color: root._surface
                                    border.color: root._border
                                    border.width: 1
                                    radius: root._radiusSm
                                }
                            }
                        }
                    }
                }

                // 窄宽页签内容（一次只显示一个）。
                Item {
                    visible: root._isNarrow
                    anchors.top: narrowTabBar.bottom
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.bottom: parent.bottom
                    anchors.margins: root._sp8

                    // 本地页签
                    ScrollView {
                        anchors.fill: parent
                        visible: root.narrowTab === 0
                        clip: true

                        TextArea {
                            readOnly: true
                            text: root.preview ? (root.preview.localContent || "") : ""
                            color: root._textPrimary
                            font.pointSize: root._fontSm
                            wrapMode: TextArea.Wrap
                            background: Rectangle {
                                color: root._surface
                                border.color: root._border
                                border.width: 1
                                radius: root._radiusSm
                            }
                        }
                    }

                    // 远端页签 — RemoteDeleted 时显示已删除提示。
                    ColumnLayout {
                        anchors.fill: parent
                        visible: root.narrowTab === 1
                        spacing: root._sp4

                        Rectangle {
                            Layout.fillWidth: true
                            Layout.fillHeight: true
                            visible: root.preview && root.preview.remoteDeleted === true
                            color: root._surface
                            border.color: root._border
                            border.width: 1
                            radius: root._radiusSm

                            AppText {
                                anchors.centerIn: parent
                                text: qsTr("远端已删除此文件")
                                dt: root.dt
                                color: root._textMuted
                                font.pointSize: root._fontSm
                                wrapMode: Text.Wrap
                                horizontalAlignment: Text.AlignHCenter
                            }
                        }

                        ScrollView {
                            Layout.fillWidth: true
                            Layout.fillHeight: true
                            visible: !(root.preview && root.preview.remoteDeleted === true)
                            clip: true

                            TextArea {
                                readOnly: true
                                text: root.preview ? (root.preview.remoteContent || "") : ""
                                color: root._textPrimary
                                font.pointSize: root._fontSm
                                wrapMode: TextArea.Wrap
                                background: Rectangle {
                                    color: root._surface
                                    border.color: root._border
                                    border.width: 1
                                    radius: root._radiusSm
                                }
                            }
                        }
                    }
                }
            }
        }

        Rectangle { Layout.fillWidth: true; height: 1; color: root._border }

        // ── Footer：三个动作按钮 ──
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 56
            color: "transparent"

            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: root._sp8
                anchors.rightMargin: root._sp8
                spacing: root._sp8

                // 保留本地
                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 36
                    radius: root._radiusSm
                    color: keepLocalHover.containsMouse ? root._surfaceVariant : root._surface
                    border.color: root._border
                    border.width: 1
                    opacity: root.conflicts.length > 0 ? 1.0 : 0.4

                    AppText {
                        anchors.centerIn: parent
                        text: qsTr("保留本地")
                        dt: root.dt
                        color: root._textPrimary
                        font.pointSize: root._fontSm
                        font.weight: Font.DemiBold
                    }

                    MouseArea {
                        id: keepLocalHover
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        enabled: root.conflicts.length > 0
                        onClicked: root.resolveAction("keep_local")
                    }
                }

                // 采用远端
                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 36
                    radius: root._radiusSm
                    color: takeRemoteHover.containsMouse ? root._surfaceVariant : root._surface
                    border.color: root._border
                    border.width: 1
                    opacity: root.conflicts.length > 0 ? 1.0 : 0.4

                    AppText {
                        anchors.centerIn: parent
                        text: qsTr("采用远端")
                        dt: root.dt
                        color: root._textPrimary
                        font.pointSize: root._fontSm
                        font.weight: Font.DemiBold
                    }

                    MouseArea {
                        id: takeRemoteHover
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        enabled: root.conflicts.length > 0
                        onClicked: root.resolveAction("take_remote")
                    }
                }

                // 手动合并后标记已解决
                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 36
                    radius: root._radiusSm
                    color: markMergedHover.containsMouse ? root._accentSoft : root._primary
                    opacity: root.conflicts.length > 0 ? 1.0 : 0.4

                    AppText {
                        anchors.centerIn: parent
                        text: qsTr("手动合并后标记已解决")
                        dt: root.dt
                        color: markMergedHover.containsMouse ? root._accentText : "#FFFFFF"
                        font.pointSize: root._fontSm
                        font.weight: Font.DemiBold
                    }

                    MouseArea {
                        id: markMergedHover
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        enabled: root.conflicts.length > 0
                        onClicked: root.resolveAction("mark_merged")
                    }
                }
            }
        }
    }
}
