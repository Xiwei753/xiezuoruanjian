// =============================================================================
// ProjectHomePage.qml — 作品首页
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 页面）
// 职责：作品封面卡网格、新建/重命名/删除操作
// 约束：
//   - 纯展示层，业务逻辑通过 signal 传递给 main.qml
//   - 不直接操作文件系统或 Core 层
//
// Issue #827 评论 2：不再复用 CardCollectionPage。
// CardCollectionPage 会自带「作品 / x 部作品 / + 新建作品」那一整套页头，
// 而 2026-10-05 草图里作品页没有这层标题栏，也没有右上角的「+ 新建作品」按钮 ——
// 主内容从内容区左上开始直接排作品卡。所以这里改成自己的左对齐滚动网格，
// 不居中、不把卡拉宽填满一列。
//
//   [作品卡] [作品卡] [ + ]
//   [作品卡] [ + ]
//
// 「+」卡是网格里的普通成员（最后一个），跟作品卡同尺寸同间距，
// 永远跟在最后一张作品卡后面；点击只打开命名框，不直接创建作品。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    required property var dt
    property var editorBackendRef: null
    property var projectBackendRef: null
    property var appState: ({})
    property var tree: []

    // Issue #827 评论 2：布局契约由上层注入（main.qml 的 window.layoutPlan），
    // 作品卡宽度读 Core 的 project_card_min_width_dp，不在 QML 写死。
    property var layoutPlan: null

    signal openProject(string projectId)
    signal createProject()
    signal renameProjectRequested(string projectId, string title)
    signal deleteProjectRequested(string projectId, string title)

    color: dt.bg

    function getProjects() {
        var projects = []
        if (!tree) return projects
        for (var i = 0; i < tree.length; i++) {
            if (tree[i].type === "project") projects.push(tree[i])
        }
        return projects
    }

    property var _cachedSummaries: ({})

    function refreshSummaries() {
        if (!projectBackendRef) return
        try {
            var jsonStr = projectBackendRef.get_project_summaries_json()
            var arr = JSON.parse(jsonStr)
            if (!arr || !Array.isArray(arr)) {
                if (editorBackendRef && editorBackendRef.log_qml) {
                    editorBackendRef.log_qml("error", "project", "project_summary_parse_failed", "summaries JSON is not an array")
                }
                return
            }
            var map = {}
            for (var i = 0; i < arr.length; i++) {
                map[arr[i].projectId] = arr[i]
            }
            root._cachedSummaries = map
        } catch (e) {
            if (editorBackendRef && editorBackendRef.log_qml) {
                editorBackendRef.log_qml("error", "project", "project_summary_parse_failed", String(e))
            }
        }
    }

    // ── 卡片尺寸：Core 的共用值，两端（作品卡 / 「+」卡）共用同一组 ──
    readonly property int _cardWidth: root.layoutPlan && root.layoutPlan.projectCardMinWidthVp > 0
                                     ? Math.round(root.layoutPlan.projectCardMinWidthVp)
                                     : 180
    // 书封面是竖向的：高 / 宽 = 4 / 3，与手绘稿一致。
    readonly property int _cardHeight: Math.round(_cardWidth * 4 / 3)
    readonly property int _gridGap: dt.sp16
    readonly property int _pagePadding: root.layoutPlan && root.layoutPlan.contentPaddingVp > 0
                                       ? Math.round(root.layoutPlan.contentPaddingVp)
                                       : dt.sp24

    // 每行放几张卡：卡宽固定，剩下多少空间就放多少，不把卡拉宽填满整列。
    readonly property int _columns: Math.max(1, Math.floor((width - _pagePadding * 2 + _gridGap) / (_cardWidth + _gridGap)))

    // Issue #827 评论 3 第 2 点：作品封面上的最后编辑时间用绝对时间，
    // 形如 26/8/11 22:37。Core 下发的是 ISO 字符串，直接画出来对不上草图，
    // 也不能用「3天前」那种相对时间 —— 封面要的是能定位到具体某天的日历时间。
    // 与 Harmony HomeScreen.formatProjectCardTime() 同格式。
    function formatProjectTime(iso) {
        if (!iso) return "";
        var d = new Date(iso);
        if (isNaN(d.getTime())) return "";
        var year = String(d.getFullYear()).slice(-2);
        var month = String(d.getMonth() + 1);
        var day = String(d.getDate());
        var hour = String(d.getHours());
        if (hour.length < 2) hour = "0" + hour;
        var minute = String(d.getMinutes());
        if (minute.length < 2) minute = "0" + minute;
        return year + "/" + month + "/" + day + " " + hour + ":" + minute;
    }

    Flickable {
        id: flick
        anchors.fill: parent
        contentWidth: width
        // 「+」卡也是网格的一员，所以内容高度由 Flow 的 implicitHeight 一并算出。
        contentHeight: Math.max(0, flow.implicitHeight) + _pagePadding * 2
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        // Issue #842: 桌面鼠标左键不能按住空白处拖页面，
        // 保留滚轮/触摸板/滚动条/触屏滚动。
        acceptedButtons: Qt.NoButton

        // 左对齐、按卡宽自然换行：不居中，也不把卡拉宽填满一列。
        Flow {
            id: flow
            x: root._pagePadding
            y: root._pagePadding
            width: root._columns * root._cardWidth + Math.max(0, root._columns - 1) * root._gridGap
            spacing: root._gridGap

            Repeater {
                model: projectModel

                // Issue #833：delegate 包一层 Item，声明与 ListModel role 同名的
                // required property，再把这些属性传给 ProjectCard。
                // Qt 6 delegate 上下文中 model 不可直接访问，运行时报
                // 'model is not defined'。required property 让 Qt 自动从 model role
                // 注入，不依赖隐式 model 对象。
                delegate: Item {
                    width: root._cardWidth
                    height: root._cardHeight

                    required property string projectId
                    required property string projectTitle
                    required property string projectLastEdited
                    required property int projectWordCount
                    required property int projectVolumeCount
                    required property int projectChapterCount

                    ProjectCard {
                        anchors.fill: parent
                        dt: root.dt
                        cardWidth: root._cardWidth
                        cardHeight: root._cardHeight
                        projectId: parent.projectId
                        title: parent.projectTitle
                        wordCount: parent.projectWordCount
                        volumeCount: parent.projectVolumeCount
                        chapterCount: parent.projectChapterCount
                        lastEdited: root.formatProjectTime(parent.projectLastEdited)
                        onClicked: root.openProject(parent.projectId)
                        onRightClicked: {
                            projectContextMenu.projectId = parent.projectId
                            projectContextMenu.projectTitle = parent.projectTitle
                            projectContextMenu.popup()
                        }
                    }
                }
            }

            // 「+」卡：网格最后一项。作品为空时它就是第一张卡。
            CreateProjectCard {
                width: root._cardWidth
                height: root._cardHeight
                dt: root.dt
                cardWidth: root._cardWidth
                cardHeight: root._cardHeight
                onClicked: root.createProject()
            }
        }
    }

    Menu {
        id: projectContextMenu
        property string projectId: ""
        property string projectTitle: ""
        MenuItem { text: qsTr("打开"); onTriggered: root.openProject(projectContextMenu.projectId) }
        MenuSeparator {}
        MenuItem { text: qsTr("重命名"); onTriggered: { renameProjectDialog.projectId = projectContextMenu.projectId; renameProjectDialog.currentTitle = projectContextMenu.projectTitle; renameProjectDialog.open() } }
        MenuItem { text: qsTr("删除"); onTriggered: root.deleteProjectRequested(projectContextMenu.projectId, projectContextMenu.projectTitle) }
    }

    Dialog {
        id: renameProjectDialog
        property string projectId: ""
        property string currentTitle: ""
        modal: true
        width: 360
        height: 208
        anchors.centerIn: Overlay.overlay
        background: Rectangle { color: dt.surface; border.color: dt.border; radius: dt.radiusXl; border.width: 1 }
        header: null
        ColumnLayout {
            anchors.fill: parent
            anchors.margins: dt.sp24
            spacing: dt.sp12

            AppText {
                dt: root.dt
                text: qsTr("重命名作品")
                color: dt.onSurface
                font.pointSize: dt.subtitlePt
                font.family: dt.fontFamily
                font.weight: Font.DemiBold
            }
            AppTextField {
                id: renameField
                Layout.fillWidth: true
                dt: root.dt
                text: renameProjectDialog.currentTitle
                placeholderText: qsTr("作品名称")
                onAccepted: renameConfirmButton.clicked()
            }
            RowLayout {
                Layout.fillWidth: true
                Item { Layout.fillWidth: true }
                AppButton {
                    text: qsTr("取消")
                    dt: root.dt
                    variant: "text"
                    onClicked: renameProjectDialog.close()
                }
                AppButton {
                    id: renameConfirmButton
                    text: qsTr("确定")
                    dt: root.dt
                    variant: "primary"
                    onClicked: {
                        var t = renameField.text.trim();
                        if (t === "") return;
                        root.renameProjectRequested(renameProjectDialog.projectId, t);
                        renameProjectDialog.close();
                    }
                }
            }
        }
        onOpened: {
            renameField.text = renameProjectDialog.currentTitle;
        }
    }

    ListModel { id: projectModel }

    function refreshProjects() {
        refreshSummaries()
        projectModel.clear()
        var projects = getProjects()
        for (var i = 0; i < projects.length; i++) {
            var p = projects[i]
            var summary = root._cachedSummaries[p.id] || {}
            // Issue #827 评论 2：卷数 / 章数 / 总字数 / 最后编辑时间
            // 全部直接取 Core summary，不再跨 FFI 重算字数。
            projectModel.append({
                projectId: p.id,
                projectTitle: p.title || qsTr("未命名作品"),
                projectWordCount: summary.totalWordCount || 0,
                projectVolumeCount: summary.volumeCount || 0,
                projectChapterCount: summary.chapterCount || 0,
                projectLastEdited: summary.updatedAt || p.updatedAt || ""
            })
        }
    }

    Component.onCompleted: refreshProjects()
    onTreeChanged: refreshProjects()
}