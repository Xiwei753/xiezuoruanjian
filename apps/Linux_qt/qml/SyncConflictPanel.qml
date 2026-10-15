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
// Issue #900：视觉迁移到 Breeze/Kirigami —— 自制按钮/胶囊改为 Controls.Button /
// Controls.ToolButton / Controls.TabBar，文字统一 Controls.Label，颜色/间距走
// Kirigami.Theme.* / Kirigami.Units.*，不再消费 DesignTokens M3 字段。
// 公开 API（属性、信号、函数、backend 调用）保持不变。
// =============================================================================

import QtQuick
import QtQuick.Controls as Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Rectangle {
    id: root

    // 调用方仍注入根 dt（保持公开 API 与构建期约定），内部不再消费其 M3 字段。
    required property var dt
    // SyncBackend 引用，用于调用 load_sync_conflict_preview / resolve_*。
    property var syncBackendRef: null
    // 当前项目 ID（由 WritingWorkspace.workspaceProjectId 传入）。
    property string projectId: ""
    // Issue #770 评论 5842877986: 冲突列表由外部（WritingWorkspace.syncConflicts）传入，
    // 面板不再自己调 list_sync_conflicts 维护第二份缓存。
    required property var conflicts
    // 外部请求选中的冲突路径（单向输入，绝不在组件内部赋值）。
    property string requestedConflictPath: ""
    // 面板内部当前选中的冲突路径（上一条/下一条、解决后选下一条只改这个）。
    property string selectedConflictPath: ""

    // 关闭只收起侧栏，冲突仍保留。
    signal closeRequested()
    // 冲突被解决后发出，通知外部刷新（RightDrawer/WritingWorkspace 可据此重载列表）。
    signal conflictsResolved()

    // 窄宽度阈值：低于此值改用页签而非并排。
    readonly property real _narrowThreshold: 500
    readonly property bool _isNarrow: root.width < _narrowThreshold

    // ── 内部状态 ──
    // load_sync_conflict_preview 返回的预览对象（{ path, kind, createdAt, localContent, remoteContent, remoteDeleted }）。
    property var preview: null
    // 当前冲突在 conflicts 数组中的索引。
    property int currentIndex: 0
    property bool loading: false
    // 窄宽页签：0=本地，1=远端。
    property int narrowTab: 0

    color: "transparent"

    // ── 数据加载 ──

    function loadPreview() {
        if (!root.syncBackendRef || !root.projectId || !root.selectedConflictPath) {
            root.preview = null;
            return;
        }
        var raw = root.syncBackendRef.load_sync_conflict_preview(root.projectId, root.selectedConflictPath);
        var resp;
        try { resp = JSON.parse(raw); } catch (e) { resp = null; }
        if (resp && resp.success && resp.data) {
            root.preview = resp.data.preview || null;
        } else {
            root.preview = null;
        }
    }

    function resolveAction(action) {
        if (!root.syncBackendRef || !root.projectId || !root.selectedConflictPath) return;
        var raw;
        if (action === "keep_local") {
            raw = root.syncBackendRef.resolve_conflict_keep_local(root.projectId, root.selectedConflictPath);
        } else if (action === "take_remote") {
            raw = root.syncBackendRef.resolve_conflict_take_remote(root.projectId, root.selectedConflictPath);
        } else if (action === "mark_merged") {
            raw = root.syncBackendRef.resolve_conflict_mark_merged(root.projectId, root.selectedConflictPath);
        } else {
            return;
        }
        var resp;
        try { resp = JSON.parse(raw); } catch (e) { resp = null; }
        if (resp && resp.success) {
            // Issue #770 评论 5842877986: 解决后不再自己 reloadConflicts，
            // 只发 conflictsResolved() 信号让外部（WritingWorkspace）刷新 syncConflicts。
            // 外部刷新后 conflicts 变化会触发 onConflictsChanged 重新选择索引。
            root.conflictsResolved();
        }
    }

    function selectIndex(idx) {
        if (idx < 0 || idx >= root.conflicts.length) return;
        root.currentIndex = idx;
        root.selectedConflictPath = root.conflicts[idx].localPath || "";
        loadPreview();
    }

    // Issue #770 评论 5842877986: 当外部 conflicts 或 requestedConflictPath 变化时，
    // 重新选择索引并加载 preview。如果 requestedConflictPath 在 conflicts 里找到就选它，
    // 否则选第一个。这是面板唯一的选择入口，不在内部调 list_sync_conflicts。
    function reselectFromConflicts() {
        var conflicts = root.conflicts || [];
        var found = -1;
        if (root.requestedConflictPath) {
            for (var i = 0; i < conflicts.length; i++) {
                if (conflicts[i].localPath === root.requestedConflictPath) {
                    found = i;
                    break;
                }
            }
        }
        if (found >= 0) {
            root.currentIndex = found;
            root.selectedConflictPath = conflicts[found].localPath || "";
        } else if (conflicts.length > 0) {
            root.currentIndex = 0;
            root.selectedConflictPath = conflicts[0].localPath || "";
        } else {
            root.currentIndex = 0;
            root.selectedConflictPath = "";
            root.preview = null;
            return;
        }
        loadPreview();
    }

    onConflictsChanged: root.reselectFromConflicts()
    onRequestedConflictPathChanged: root.reselectFromConflicts()
    onProjectIdChanged: {
        // Issue #770 评论 5842877986: 切换作品后冲突列表由外部统一刷新，
        // 面板只清理 preview，不再自己调 list_sync_conflicts。
        root.preview = null;
    }
    Component.onCompleted: root.reselectFromConflicts()

    // ── 布局 ──

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // ── Header：文件名 + 第几个/共几个 + 关闭 ──
        RowLayout {
            Layout.fillWidth: true
            Layout.preferredHeight: Kirigami.Units.gridUnit * 2
            Layout.leftMargin: Kirigami.Units.largeSpacing
            Layout.rightMargin: Kirigami.Units.smallSpacing
            spacing: Kirigami.Units.smallSpacing

            Controls.Label {
                Layout.fillWidth: true
                text: root.selectedConflictPath ? root.selectedConflictPath : qsTr("无冲突")
                color: Kirigami.Theme.textColor
                font.bold: true
                elide: Text.ElideMiddle
            }

            Controls.Label {
                Layout.alignment: Qt.AlignVCenter
                visible: root.conflicts.length > 0
                text: (root.currentIndex + 1) + " / " + root.conflicts.length
                color: Kirigami.Theme.disabledTextColor
            }

            // 关闭按钮 — 只收起侧栏，冲突仍保留。
            Controls.ToolButton {
                icon.name: "dialog-close"
                text: qsTr("关闭")
                display: Controls.AbstractButton.IconOnly
                onClicked: root.closeRequested()
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 1
            color: Kirigami.Theme.disabledTextColor
        }

        // ── Content：本地/远端预览 ──
        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true

            // 空状态：无冲突。
            ColumnLayout {
                anchors.centerIn: parent
                spacing: Kirigami.Units.largeSpacing
                visible: root.conflicts.length === 0 && !root.loading

                Kirigami.Icon {
                    Layout.alignment: Qt.AlignHCenter
                    source: "dialog-ok"
                    implicitWidth: Kirigami.Units.iconSizes.huge
                    implicitHeight: Kirigami.Units.iconSizes.huge
                }
                Controls.Label {
                    Layout.alignment: Qt.AlignHCenter
                    text: qsTr("没有未解决的冲突")
                    color: Kirigami.Theme.textColor
                    font.bold: true
                }
            }

            // 加载中。
            Controls.Label {
                anchors.centerIn: parent
                visible: root.loading
                text: qsTr("加载中…")
                color: Kirigami.Theme.disabledTextColor
            }

            // 有冲突时展示预览。
            Item {
                anchors.fill: parent
                visible: root.conflicts.length > 0 && !root.loading

                // 窄宽页签栏。
                Controls.TabBar {
                    id: narrowTabBar
                    visible: root._isNarrow
                    anchors.top: parent.top
                    anchors.left: parent.left
                    anchors.right: parent.right
                    currentIndex: root.narrowTab
                    onCurrentIndexChanged: root.narrowTab = currentIndex

                    Controls.TabButton { text: qsTr("本地") }
                    Controls.TabButton { text: qsTr("远端") }
                }

                // 并排展示（宽屏）。
                RowLayout {
                    id: sideBySide
                    visible: !root._isNarrow
                    anchors.fill: parent
                    anchors.margins: Kirigami.Units.smallSpacing
                    spacing: Kirigami.Units.smallSpacing

                    // 本地正文
                    ColumnLayout {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        spacing: Kirigami.Units.smallSpacing

                        Controls.Label {
                            text: qsTr("本地")
                            color: Kirigami.Theme.disabledTextColor
                            font.bold: true
                        }

                        Controls.ScrollView {
                            Layout.fillWidth: true
                            Layout.fillHeight: true
                            clip: true
                            // Issue #782 评论 5855709706: 桌面鼠标左键不能按住空白处拖页面。
                            Component.onCompleted: {
                                if (contentItem) contentItem.acceptedButtons = Qt.NoButton
                            }

                            Controls.TextArea {
                                readOnly: true
                                text: root.preview ? (root.preview.localContent || "") : ""
                                color: Kirigami.Theme.textColor
                                wrapMode: TextEdit.Wrap
                                background: Rectangle {
                                    color: Kirigami.Theme.backgroundColor
                                    border.color: Kirigami.Theme.disabledTextColor
                                    border.width: 1
                                    radius: Kirigami.Units.smallSpacing
                                }
                            }
                        }
                    }

                    // 远端正文 / 已删除提示
                    ColumnLayout {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        spacing: Kirigami.Units.smallSpacing

                        Controls.Label {
                            text: qsTr("远端")
                            color: Kirigami.Theme.disabledTextColor
                            font.bold: true
                        }

                        // RemoteDeleted：显示"远端已删除此文件"，不伪造空字符串。
                        ColumnLayout {
                            Layout.fillWidth: true
                            Layout.fillHeight: true
                            visible: root.preview && root.preview.remoteDeleted === true

                            Rectangle {
                                Layout.fillWidth: true
                                Layout.fillHeight: true
                                color: Kirigami.Theme.backgroundColor
                                border.color: Kirigami.Theme.disabledTextColor
                                border.width: 1
                                radius: Kirigami.Units.smallSpacing

                                Controls.Label {
                                    anchors.centerIn: parent
                                    width: parent.width - Kirigami.Units.largeSpacing
                                    text: qsTr("远端已删除此文件")
                                    color: Kirigami.Theme.disabledTextColor
                                    wrapMode: Text.Wrap
                                    horizontalAlignment: Text.AlignHCenter
                                }
                            }
                        }

                        // BothChanged：显示远端 snapshot 正文。
                        Controls.ScrollView {
                            Layout.fillWidth: true
                            Layout.fillHeight: true
                            visible: !(root.preview && root.preview.remoteDeleted === true)
                            clip: true
                            // Issue #782 评论 5855709706: 桌面鼠标左键不能按住空白处拖页面。
                            Component.onCompleted: {
                                if (contentItem) contentItem.acceptedButtons = Qt.NoButton
                            }

                            Controls.TextArea {
                                readOnly: true
                                text: root.preview ? (root.preview.remoteContent || "") : ""
                                color: Kirigami.Theme.textColor
                                wrapMode: TextEdit.Wrap
                                background: Rectangle {
                                    color: Kirigami.Theme.backgroundColor
                                    border.color: Kirigami.Theme.disabledTextColor
                                    border.width: 1
                                    radius: Kirigami.Units.smallSpacing
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
                    anchors.margins: Kirigami.Units.smallSpacing

                    // 本地页签
                    Controls.ScrollView {
                        anchors.fill: parent
                        visible: root.narrowTab === 0
                        clip: true
                        // Issue #782 评论 5855709706: 桌面鼠标左键不能按住空白处拖页面。
                        Component.onCompleted: {
                            if (contentItem) contentItem.acceptedButtons = Qt.NoButton
                        }

                        Controls.TextArea {
                            readOnly: true
                            text: root.preview ? (root.preview.localContent || "") : ""
                            color: Kirigami.Theme.textColor
                            wrapMode: TextEdit.Wrap
                            background: Rectangle {
                                color: Kirigami.Theme.backgroundColor
                                border.color: Kirigami.Theme.disabledTextColor
                                border.width: 1
                                radius: Kirigami.Units.smallSpacing
                            }
                        }
                    }

                    // 远端页签 — RemoteDeleted 时显示已删除提示。
                    ColumnLayout {
                        anchors.fill: parent
                        visible: root.narrowTab === 1
                        spacing: Kirigami.Units.smallSpacing

                        Rectangle {
                            Layout.fillWidth: true
                            Layout.fillHeight: true
                            visible: root.preview && root.preview.remoteDeleted === true
                            color: Kirigami.Theme.backgroundColor
                            border.color: Kirigami.Theme.disabledTextColor
                            border.width: 1
                            radius: Kirigami.Units.smallSpacing

                            Controls.Label {
                                anchors.centerIn: parent
                                width: parent.width - Kirigami.Units.largeSpacing
                                text: qsTr("远端已删除此文件")
                                color: Kirigami.Theme.disabledTextColor
                                wrapMode: Text.Wrap
                                horizontalAlignment: Text.AlignHCenter
                            }
                        }

                        Controls.ScrollView {
                            Layout.fillWidth: true
                            Layout.fillHeight: true
                            visible: !(root.preview && root.preview.remoteDeleted === true)
                            clip: true
                            // Issue #782 评论 5855709706: 桌面鼠标左键不能按住空白处拖页面。
                            Component.onCompleted: {
                                if (contentItem) contentItem.acceptedButtons = Qt.NoButton
                            }

                            Controls.TextArea {
                                readOnly: true
                                text: root.preview ? (root.preview.remoteContent || "") : ""
                                color: Kirigami.Theme.textColor
                                wrapMode: TextEdit.Wrap
                                background: Rectangle {
                                    color: Kirigami.Theme.backgroundColor
                                    border.color: Kirigami.Theme.disabledTextColor
                                    border.width: 1
                                    radius: Kirigami.Units.smallSpacing
                                }
                            }
                        }
                    }
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 1
            color: Kirigami.Theme.disabledTextColor
        }

        // ── Footer：三个动作按钮 ──
        RowLayout {
            Layout.fillWidth: true
            Layout.margins: Kirigami.Units.smallSpacing
            spacing: Kirigami.Units.smallSpacing

            // 保留本地
            Controls.Button {
                Layout.fillWidth: true
                text: qsTr("保留本地")
                enabled: root.conflicts.length > 0
                onClicked: root.resolveAction("keep_local")
            }

            // 采用远端
            Controls.Button {
                Layout.fillWidth: true
                text: qsTr("采用远端")
                enabled: root.conflicts.length > 0
                onClicked: root.resolveAction("take_remote")
            }

            // 手动合并后标记已解决
            Controls.Button {
                Layout.fillWidth: true
                text: qsTr("手动合并后标记已解决")
                highlighted: true
                enabled: root.conflicts.length > 0
                onClicked: root.resolveAction("mark_merged")
            }
        }
    }
}
