// =============================================================================
// SyncPage.qml — 同步页面
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 页面）
// 职责：同步配置展示、手动同步触发、同步诊断、错误展示
// 约束：不直接操作 Git 或文件系统，业务动作全部委托 syncBackend 兼容入口
// Issue #900：视觉全面迁移到 KDE 官方组件体系（Breeze Controls +
// Kirigami Addons FormCard），不再使用 AppCard/AppTextField/AppButton/
// AppText/StatusPill 等自制 M3 皮肤组件。业务逻辑（消息映射、状态归并、
// 冲突分组、backend 调用与反馈）原样保留。
// =============================================================================

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.kirigamiaddons.formcard as FormCard

ColumnLayout {
    id: root

    // KDE FormCard 默认内容宽度上限（Kirigami.Units.gridUnit * 30）。
    // 非卡片内容（状态行、按钮条、日志框）显式用它约束并居中，保证与
    // KdeSettingsPage 中其它 FormCard 左对齐同一条边。
    readonly property int cardWidth: Kirigami.Units.gridUnit * 30

    spacing: Kirigami.Units.largeSpacing

    property int lastSyncResultLen: -1
    property double lastSyncStatusLogTime: 0
    property var syncBackendRef: null
    // Issue #754 评论 5814866116 改动4: has_workspace 改读 workspaceBackendRef，
    // SyncBackend 不再暴露 has_workspace property。
    property var workspaceBackendRef: null
    property var beforeSyncHook: null
    signal settingsChanged()

    property string activeOperationId: ""
    property string activeOperationKind: ""

    // 结果区与原始诊断文本（原 TextArea/TextEdit 的文本源）。
    property string resultText: ""
    property string rawErrorText: ""
    property bool showRawError: false

    // Issue #762 评论 5826175490 第 4 点：跨作品全局冲突入口。
    // allSyncConflicts 是 Core list_all_sync_conflicts() 的原始扁平列表
    // （projectId + projectTitle + conflict，只含未解决冲突）；
    // conflictGroups 按作品分组；conflictRows 把分组展平成
    // "作品分组头 + 该作品每条冲突"，供单个 Repeater 渲染。
    property var allSyncConflicts: []
    property var conflictGroups: []
    property var conflictRows: []
    readonly property int conflictTotalCount: allSyncConflicts.length
    signal openConflict(string targetKind, string projectId, string path)

    // Issue #902 第二十一轮：App 冲突（星图等应用级数据）预览/解决面板。
    // App 冲突在 SyncPage 内部以窗口级 Popup 展示，不走 openWriting 跳转。
    property string appConflictPath: ""
    // App 冲突列表（从 allSyncConflicts 筛 targetKind==="app"，path 映射为 localPath）。
    property var appConflictList: []

    // Local reactive sync state
    property string currentSyncStatus: "not_configured"
    property bool currentSyncInProgress: false
    property string currentSyncOperationState: ""

    function syncMessageForKey(key, args) {
        if (!key) return ""
        var msg = ""
        switch (key) {
        case "sync.block.no_workspace": msg = qsTr("未打开工作区"); break;
        case "sync.block.disabled": msg = qsTr("同步已禁用"); break;
        case "sync.block.remote_url_missing": msg = qsTr("未设置远程仓库地址"); break;
        case "sync.block.token_missing": msg = qsTr("未设置访问 Token"); break;
        case "sync.block.invalid_directory": msg = qsTr("工作区目录无效"); break;
        case "sync.phase.diagnose": msg = qsTr("正在诊断..."); break;
        case "sync.phase.dry_run": msg = qsTr("正在试运行..."); break;
        case "sync.phase.syncing": msg = qsTr("正在同步..."); break;
        case "sync.phase.background_syncing": msg = qsTr("正在后台同步..."); break;
        case "sync.phase.github_init": msg = qsTr("正在初始化 GitHub 连接..."); break;
        case "sync.result.diagnose_success": msg = qsTr("诊断完成"); break;
        case "sync.result.diagnose_failed": msg = qsTr("诊断失败"); break;
        case "sync.result.dry_run_summary": msg = qsTr("试运行完成"); break;
        case "sync.result.dry_run_failed": msg = qsTr("试运行失败"); break;
        case "sync.result.success_summary": msg = qsTr("同步完成"); break;
        case "sync.result.latest_wins_summary": msg = qsTr("同步完成（以最新版本为准）"); break;
        case "sync.result.no_changes_summary": msg = qsTr("无需同步，本地与远端一致"); break;
        case "sync.result.conflict_summary": msg = qsTr("同步完成，存在冲突文件"); break;
        case "sync.result.partial_conflict_summary": msg = qsTr("部分同步，存在正文冲突"); break;
        case "sync.result.dirty_repo_blocked": msg = qsTr("同步被阻止：本地有未提交的更改"); break;
        case "sync.result.branch_recovered_summary": msg = qsTr("分支已恢复并同步完成"); break;
        case "sync.result.generic_error": msg = qsTr("同步出错"); break;
        case "sync.result.save_config_success": msg = qsTr("配置已保存"); break;
        case "sync.result.save_config_failed": msg = qsTr("保存配置失败"); break;
        case "sync.result.clone_success_init_failed": msg = qsTr("克隆成功但初始化失败"); break;
        case "sync.result.push_failed_save_config_failed": msg = qsTr("推送失败且保存配置失败"); break;
        case "sync.result.push_failed": msg = qsTr("推送失败"); break;
        case "sync.result.clone_init_success": msg = qsTr("克隆并初始化成功"); break;
        case "sync.result.clone_failed": msg = qsTr("克隆失败"); break;
        case "sync.result.remote_configured_sync_success": msg = qsTr("远端已配置，同步成功"); break;
        case "sync.result.no_conflict_files": msg = qsTr("无冲突文件"); break;
        case "sync.result.more_files_count": msg = qsTr("还有更多文件"); break;
        case "sync.result.git_repo_not_workspace": msg = qsTr("该目录已是 Git 仓库但不是工作区"); break;
        case "sync.result.directory_not_empty_not_workspace": msg = qsTr("目录非空且不是工作区"); break;
        case "sync.result.configured_not_tested": msg = qsTr("已配置，尚未测试"); break;
        case "sync.status.already_running": msg = qsTr("同步正在运行中"); break;
        case "error.io": msg = qsTr("文件读写失败"); break;
        case "error.json": msg = qsTr("数据格式异常"); break;
        case "error.project_not_found": msg = qsTr("作品不存在"); break;
        case "error.volume_not_found": msg = qsTr("卷不存在"); break;
        case "error.chapter_not_found": msg = qsTr("章节不存在"); break;
        case "error.empty_overwrite_blocked": msg = qsTr("已阻止空内容覆盖"); break;
        case "error.not_implemented": msg = qsTr("功能尚未实现"); break;
        case "error.refuse_delete_root": msg = qsTr("拒绝删除根目录"); break;
        case "error.invalid_delete_target": msg = qsTr("删除目标无效"); break;
        case "error.sync_conflict": msg = qsTr("同步冲突，请手动处理冲突文件后重试"); break;
        case "error.sync_failed": msg = qsTr("同步失败，请检查网络和配置"); break;
        case "error.other": msg = qsTr("操作失败"); break;
        case "error.core_error": msg = qsTr("核心模块错误"); break;
        case "error.clipboard_unavailable": msg = qsTr("剪贴板不可用"); break;
        case "error.json_parse": msg = qsTr("数据解析失败"); break;
        case "error.empty_title": msg = qsTr("标题不能为空"); break;
        case "error.sync_diagnose_panic": msg = qsTr("诊断过程异常终止"); break;
        case "error.sync_dry_run_panic": msg = qsTr("试运行过程异常终止"); break;
        case "error.sync_panic": msg = qsTr("同步过程异常终止"); break;
        case "error.load_sync_config_failed": msg = qsTr("加载同步配置失败"); break;
        case "error.core_not_initialized": msg = qsTr("核心模块未初始化"); break;
        case "error.parse_json_failed": msg = qsTr("解析数据失败"); break;
        case "error.save_sync_config_failed": msg = qsTr("保存同步配置失败"); break;
        case "error.save_sync_secrets_failed": msg = qsTr("保存同步密钥失败"); break;
        case "chapter.deleted_remotely_refreshed": msg = qsTr("章节已在远端被删除，已刷新列表"); break;
        default: msg = qsTr("同步出错"); if (typeof window !== "undefined" && typeof window.debugWarn === "function") window.debugWarn("sync", "unmapped_message_key", key); break;
        }
        if (args) {
            for (var k in args) {
                msg = msg.replace("{" + k + "}", args[k])
            }
        }
        return msg
    }

    function updateSyncResultText() {
        if (root.syncBackendRef) {
            try {
                var obj = JSON.parse(root.currentSyncOperationState);
                if (root.activeOperationId === "" || obj.operation_id === root.activeOperationId) {
                    var text = ""
                    if (obj.phase_key) {
                        text += syncMessageForKey(obj.phase_key, obj.summary_args) + "\n"
                    }
                    if (obj.summary_key) {
                        text += syncMessageForKey(obj.summary_key, obj.summary_args)
                    }

                    // Display counts if it's a summary
                    if (obj.counts && (obj.counts.uploaded > 0 || obj.counts.downloaded > 0 || obj.counts.local_deleted > 0 || obj.counts.remote_deleted > 0 || obj.counts.conflicts > 0)) {
                        text += "\n\n" + qsTr("上传: ") + obj.counts.uploaded
                        text += "\n" + qsTr("下载: ") + obj.counts.downloaded
                        text += "\n" + qsTr("本地删除: ") + obj.counts.local_deleted
                        text += "\n" + qsTr("远端删除: ") + obj.counts.remote_deleted
                        if (obj.counts.overwritten > 0) text += "\n" + qsTr("覆盖: ") + obj.counts.overwritten
                        if (obj.counts.conflicts > 0) text += "\n" + qsTr("冲突: ") + obj.counts.conflicts
                    }

                    if (obj.summary_args && obj.summary_args.conflict_files) {
                        text += "\n\n" + qsTr("冲突文件:") + "\n  - " + obj.summary_args.conflict_files
                    }

                    root.resultText = text;

                    if (obj.raw_error) {
                        root.rawErrorText = obj.raw_error
                        root.showRawError = true
                    } else {
                        root.rawErrorText = ""
                        root.showRawError = false
                    }
                } else {
                    root.resultText = qsTr("同步状态解析失败，请复制诊断信息反馈");
                    if (typeof window !== "undefined" && typeof window.debugWarn === "function") window.debugWarn("sync", "operation_state_parse_failed", root.currentSyncOperationState);
                }
            } catch(e) {
                root.resultText = qsTr("同步状态解析失败，请复制诊断信息反馈");
                if (typeof window !== "undefined" && typeof window.debugWarn === "function") window.debugWarn("sync", "operation_state_json_error", e.toString());
            }
        }
    }

    function showConfigFeedback(message, isError) {
        syncConfigFeedback.message = message
        syncConfigFeedback.isError = isError
        syncConfigFeedback.visible = true
        configFeedbackTimer.restart()
    }

    function refreshLocalSyncState() {
        if (root.syncBackendRef) {
            root.currentSyncStatus = root.syncBackendRef.sync_status || "not_configured";
            root.currentSyncInProgress = root.syncBackendRef.sync_in_progress || false;
            root.currentSyncOperationState = root.syncBackendRef.sync_operation_state || "";
            autoSyncSwitch.checked = root.syncBackendRef.sync_auto_sync || false;
            syncIntervalSpin.value = Math.round((root.syncBackendRef.sync_interval || 300) / 60);
            root.updateSyncResultText();
            // 同步失败时，如果结果区为空，则显示错误信息
            if (root.isFailureStatus(root.currentSyncStatus) && root.resultText.trim() === "") {
                var opState = root.currentSyncOperationState.trim();
                var parsed = null;
                try { parsed = JSON.parse(opState); } catch(e) {}
                if (parsed && parsed.summary_key) {
                    root.resultText = syncMessageForKey(parsed.summary_key, parsed.summary_args);
                } else if (parsed && parsed.phase_key) {
                    root.resultText = syncMessageForKey(parsed.phase_key, parsed.summary_args);
                } else {
                    root.resultText = qsTr("同步失败");
                }
                if (parsed && parsed.raw_error) {
                    root.rawErrorText = parsed.raw_error;
                    root.showRawError = true;
                }
            }
        }
    }

    // ── Issue #762 评论 5826175490 第 4 点：跨作品全局冲突 ──

    /// 读取 Core 的全局冲突列表（只含未解决冲突），按作品分组后展平成渲染行。
    /// 冲突是持久状态，与"当前有没有在跑一轮同步"无关，因此页面打开、
    /// 状态变化（sync_conflicts_changed）时都调，不依赖同步结束。
    function refreshAllSyncConflicts() {
        var sb = root.syncBackendRef
        if (!sb) {
            root.allSyncConflicts = []
            root.conflictGroups = []
            root.conflictRows = []
            root.appConflictList = []
            return
        }
        var raw = sb.list_all_sync_conflicts()
        var resp = null
        try { resp = JSON.parse(raw) } catch (e) { resp = null }
        var flat = (resp && resp.success && resp.data && resp.data.conflicts) ? resp.data.conflicts : []
        root.allSyncConflicts = flat
        root.conflictGroups = root.groupConflictsByProject(flat)
        root.conflictRows = root.buildConflictRows(root.conflictGroups)
        root.appConflictList = root.buildAppConflictList(flat)
    }

    /// 把扁平冲突列表按 targetKind + projectId 分组，保留 Core 返回的作品标题。
    /// Issue #902 第二十轮：app target（projectId 为空）与 project target 互不混组。
    function groupConflictsByProject(flat) {
        var groups = []
        for (var i = 0; i < flat.length; i++) {
            var entry = flat[i]
            if (!entry) continue
            var groupKey = entry.targetKind + "|" + entry.projectId
            var group = null
            for (var j = 0; j < groups.length; j++) {
                if (groups[j]._groupKey === groupKey) { group = groups[j]; break }
            }
            if (!group) {
                group = {
                    _groupKey: groupKey,
                    targetKind: entry.targetKind || "project",
                    projectId: entry.projectId,
                    projectTitle: entry.projectTitle,
                    conflicts: []
                }
                groups.push(group)
            }
            group.conflicts.push(entry)
        }
        return groups
    }

    /// 展平成渲染行：每个作品一行分组头，其下每条冲突一行可点击项。
    /// Issue #902 第二十轮：行数据携带 targetKind，点击时一路传给 openConflict。
    function buildConflictRows(groups) {
        var rows = []
        for (var i = 0; i < groups.length; i++) {
            var group = groups[i]
            rows.push({
                kind: "project",
                targetKind: group.targetKind,
                projectId: group.projectId,
                projectTitle: group.projectTitle,
                conflictCount: group.conflicts.length,
                localPath: ""
            })
            for (var j = 0; j < group.conflicts.length; j++) {
                rows.push({
                    kind: "conflict",
                    targetKind: group.targetKind,
                    projectId: group.projectId,
                    projectTitle: group.projectTitle,
                    conflictCount: group.conflicts.length,
                    localPath: group.conflicts[j].path || ""
                })
            }
        }
        return rows
    }

    /// Issue #902 第二十轮：构造 App 冲突列表，把 AllSyncConflictEntryDto.path
    /// 映射为 SyncConflictPanel 期望的 localPath。
    function buildAppConflictList(flat) {
        var list = []
        for (var i = 0; i < flat.length; i++) {
            var entry = flat[i]
            if (!entry) continue
            if ((entry.targetKind || "project") === "app") {
                list.push({ localPath: entry.path || "", kind: entry.kind || "" })
            }
        }
        return list
    }
    function statusKind() {
        var s = root.currentSyncStatus
        // Issue #782 评论 5856119763: no_changes / latest_wins_applied 也是 Core 的
        // 合法同步成功终态，和 success 一样归到 success pill，不要显示成普通 info。
        if (s === "success" || s === "no_changes" || s === "latest_wins_applied") return "success"
        if (s === "syncing") return "warning"
        if (s === "partial_conflict") return "warning"
        if (root.isFailureStatus(s)) return "error"
        return "info"
    }

    function statusColor() {
        var k = root.statusKind()
        if (k === "success") return Kirigami.Theme.positiveTextColor
        if (k === "warning") return Kirigami.Theme.neutralTextColor
        if (k === "error") return Kirigami.Theme.negativeTextColor
        return Kirigami.Theme.highlightColor
    }

    function isFailureStatus(s) {
        return s === "error" || s === "conflict" || s === "partial_conflict" || s === "recoverable_error" || s === "fatal_error" || s === "auth_failed" || s === "network_failed" || s === "token_missing" || s === "token_invalid" || s === "token_permission_denied" || s === "repo_not_found_or_no_permission" || s === "branch_missing"
    }

    function isConflictStatus(s) {
        return s === "conflict" || s === "partial_conflict"
    }

    function statusText() {
        var s = root.currentSyncStatus
        if (s === "success") return qsTr("已同步")
        // Issue #782 评论 5856119763: 补齐 Core 合法同步终态的展示映射。
        // no_changes / latest_wins_applied 之前会一路掉到"同步"，不是完成状态。
        if (s === "no_changes") return qsTr("无需同步")
        if (s === "latest_wins_applied") return qsTr("已同步（已应用较新版本）")
        if (s === "syncing") return qsTr("同步中")
        if (s === "conflict") return qsTr("同步冲突")
        if (s === "partial_conflict") return qsTr("部分同步，存在正文冲突")
        if (s === "dry_run_success") return qsTr("检查成功")
        if (s === "diagnostics_success") return qsTr("诊断成功")
        if (s === "token_missing") return qsTr("未设置 Token")
        if (s === "token_invalid") return qsTr("GitHub token 无效或已过期。请检查 token 是否正确。")
        if (s === "token_permission_denied") return qsTr("GitHub token 权限不足。请给该 token 勾选目标仓库，并授予 Contents: Read and write。")
        if (s === "repo_not_found_or_no_permission") return qsTr("仓库不存在或无权限")
        if (s === "remote_branch_missing" || s === "branch_missing") return qsTr("远程分支不存在")
        if (s === "network_failed") return qsTr("网络连接失败")
        if (root.isFailureStatus(s)) return qsTr("同步失败")
        // 同步状态枚举显示：未配置、已配置未测试
        if (s === "not_configured") return qsTr("未配置")
        if (s === "no_workspace") return qsTr("未打开工作区")
        if (s === "configured_not_tested") return qsTr("已配置")
        if (root.syncBackendRef && root.syncBackendRef.sync_enabled) return qsTr("同步")
        return qsTr("配置同步")
    }

    Connections {
        target: root.syncBackendRef
        function onSync_action_completed() {
            var stateStr = (root.syncBackendRef && root.syncBackendRef.sync_operation_state) || "";
            if (typeof window !== "undefined" && typeof window.debugLog === "function") {
                window.debugLog("sync", "action_completed_callback", "resultLength=" + stateStr.length)
            }
            root.refreshLocalSyncState();
        }
        function onSync_status_changed() {
            var stateStr = (root.syncBackendRef && root.syncBackendRef.sync_operation_state) || "";
            var resLen = stateStr.length
            var now = Date.now()
            var shouldLog = true
            if (resLen === root.lastSyncResultLen && now - root.lastSyncStatusLogTime < 5000) shouldLog = false
            if (shouldLog) {
                root.lastSyncResultLen = resLen
                root.lastSyncStatusLogTime = now
                if (typeof window !== "undefined" && typeof window.debugLog === "function") {
                    window.debugLog("sync", "status_changed_callback", "resultLength=" + resLen)
                }
            }
            root.refreshLocalSyncState();
        }
        function onSync_config_changed() {
            root.refreshLocalSyncState();
        }
    }

    // ── 状态行 ──
    RowLayout {
        Layout.fillWidth: true
        Layout.maximumWidth: root.cardWidth
        Layout.alignment: Qt.AlignHCenter
        spacing: Kirigami.Units.smallSpacing

        // 状态徽标（Breeze 语义色）
        Rectangle {
            implicitWidth: statusLabel.implicitWidth + Kirigami.Units.largeSpacing
            implicitHeight: statusLabel.implicitHeight + Kirigami.Units.smallSpacing
            radius: height / 2
            color: root.statusColor()

            Label {
                id: statusLabel
                anchors.centerIn: parent
                text: root.statusText()
                color: Kirigami.Theme.backgroundColor
                font.bold: true
            }
        }

        // Issue #762 评论 5826175490 第 4 点：同步状态区域显示跨作品待处理冲突数。
        // 只反映持久冲突状态，不依赖本轮同步是否结束。
        Label {
            id: pendingConflictsCount
            visible: root.conflictTotalCount > 0
            text: qsTr("待处理冲突 %1").arg(root.conflictTotalCount)
            color: Kirigami.Theme.negativeTextColor
            font.bold: true
            Layout.alignment: Qt.AlignVCenter
        }

        Item { Layout.fillWidth: true }
    }

    // ── 配置卡片 ──
    FormCard.FormCard {
        Layout.fillWidth: true
        Layout.maximumWidth: root.cardWidth
        Layout.alignment: Qt.AlignHCenter

        FormCard.FormTextFieldDelegate {
            id: urlField
            label: qsTr("远程仓库地址")
            text: (root.syncBackendRef ? root.syncBackendRef.sync_remote_url : "")
            placeholderText: "https://github.com/user/repo"
        }

        FormCard.FormTextFieldDelegate {
            id: tokenField
            label: qsTr("访问 Token")
            placeholderText: (root.syncBackendRef ? root.syncBackendRef.has_sync_token : false) ? qsTr("已设置（输入新 Token 以覆盖）") : qsTr("请输入 GitHub Personal Access Token")
            echoMode: TextInput.Password
        }

        FormCard.FormSwitchDelegate {
            id: autoSyncSwitch
            text: qsTr("自动同步")
            description: qsTr("启用后按设定间隔自动执行同步")
            onToggled: {
                if (root.syncBackendRef) {
                    root.syncBackendRef.sync_auto_sync = autoSyncSwitch.checked
                }
            }
        }

        FormCard.FormSpinBoxDelegate {
            id: syncIntervalSpin
            visible: autoSyncSwitch.checked
            label: qsTr("同步间隔")
            from: 1
            to: 60
            stepSize: 1
            textFromValue: function(value) { return value + qsTr(" 分钟") }
            onValueModified: {
                if (root.syncBackendRef) {
                    root.syncBackendRef.sync_interval = Math.round(value) * 60
                }
            }
        }
    }

    // ── 动作按钮条（Breeze Buttons） ──
    Flow {
        Layout.fillWidth: true
        Layout.maximumWidth: root.cardWidth
        Layout.alignment: Qt.AlignHCenter
        spacing: Kirigami.Units.smallSpacing

        Button {
            text: qsTr("保存配置")
            highlighted: true
            onClicked: {
                if (typeof window !== "undefined" && typeof window.debugLog === "function") {
                    var hasNewToken = tokenField.text.trim().length > 0
                    window.debugLog("sync", "save_config_clicked", "url=" + urlField.text + ", hasNewToken=" + hasNewToken)
                }
                if (!root.syncBackendRef) return
                root.syncBackendRef.sync_remote_url = urlField.text
                if (tokenField.text.trim().length > 0) {
                    root.syncBackendRef.set_sync_token(tokenField.text.trim())
                    tokenField.text = ""
                }
                root.syncBackendRef.sync_enabled = true
                root.syncBackendRef.sync_backend_type = "github_api"

                var success = root.syncBackendRef.save_sync_config()
                if (typeof window !== "undefined" && typeof window.debugLog === "function") {
                    window.debugLog("sync", "save_config_finished", "success=" + success)
                }
                if (success) {
                    root.syncBackendRef.load_sync_config()
                    root.refreshLocalSyncState()
                    tokenField.text = ""
                    root.settingsChanged()
                    root.showConfigFeedback(qsTr("配置已保存"), false)
                } else {
                    root.resultText = root.syncBackendRef.sync_operation_state || qsTr("保存配置失败")
                    root.showConfigFeedback(qsTr("保存配置失败"), true)
                }
            }
        }

        Button {
            text: qsTr("执行同步")
            enabled: root.syncBackendRef && root.syncBackendRef.sync_can_run
            onClicked: {
                if (typeof window !== "undefined" && typeof window.debugLog === "function") window.debugLog("sync", "perform_sync_clicked", "")
                // 先保存当前 UI 配置
                if (root.syncBackendRef) {
                    root.syncBackendRef.sync_remote_url = urlField.text
                    if (tokenField.text.trim().length > 0) {
                        root.syncBackendRef.set_sync_token(tokenField.text.trim())
                        tokenField.text = ""
                    }
                    root.syncBackendRef.save_sync_config()
                    root.syncBackendRef.load_sync_config()
                }
                // 再执行同步
                if (typeof root.beforeSyncHook === "function") {
                    if (!root.beforeSyncHook()) {
                        root.resultText = qsTr("同步已取消：保存当前章节失败，请检查编辑器内容后重试")
                        return
                    }
                }
                root.resultText = qsTr("正在同步...\n正在拉取远端清单\n正在比较本地和远端\n正在下载远端较新文件\n正在上传本地较新文件")
                if (root.syncBackendRef) {
                    var opId = root.syncBackendRef.perform_sync()
                    root.activeOperationId = opId
                    root.activeOperationKind = "sync"
                    root.refreshLocalSyncState()
                    // 如果 opId 为空，显示即时错误
                    if (!opId || opId.length === 0) {
                        root.resultText = root.syncBackendRef.sync_operation_state || qsTr("同步启动失败")
                    }
                }
            }
        }

        Button {
            text: qsTr("运行诊断")
            enabled: root.syncBackendRef && root.workspaceBackendRef && root.workspaceBackendRef.has_workspace && !(root.syncBackendRef.sync_in_progress || false)
            onClicked: {
                if (typeof window !== "undefined" && typeof window.debugLog === "function") window.debugLog("sync", "perform_diagnostics_clicked", "")
                root.resultText = qsTr("正在诊断...")
                if (root.syncBackendRef) {
                    var opId = root.syncBackendRef.perform_sync_diagnostics()
                    root.activeOperationId = opId
                    root.activeOperationKind = "dry_run"
                    // 如果 opId 为空，显示即时错误
                    if (!opId || opId.length === 0) {
                        root.resultText = root.syncBackendRef.sync_operation_state || qsTr("诊断启动失败")
                    }
                }
            }
        }

        Button {
            text: qsTr("打开工作区目录")
            visible: root.syncBackendRef && root.workspaceBackendRef && root.workspaceBackendRef.has_workspace
            onClicked: if (root.syncBackendRef) root.syncBackendRef.open_workspace_dir()
        }

        Button {
            text: qsTr("复制冲突信息")
            visible: root.syncBackendRef && root.isConflictStatus(root.currentSyncStatus)
            onClicked: if (root.syncBackendRef) root.syncBackendRef.copy_text_to_clipboard(root.resultText)
        }
    }

    Label {
        id: syncBlockReason
        Layout.fillWidth: true
        Layout.maximumWidth: root.cardWidth
        Layout.alignment: Qt.AlignHCenter
        visible: root.syncBackendRef && !root.syncBackendRef.sync_can_run && root.syncBackendRef.sync_block_reason.length > 0
        text: root.syncBackendRef ? root.syncBackendRef.sync_block_reason : ""
        color: Kirigami.Theme.disabledTextColor
        wrapMode: Text.Wrap
    }

    // pending manual 状态展示：当前自动同步运行时点击，显示"当前同步完成后将再次同步"
    Label {
        id: manualSyncPendingHint
        Layout.fillWidth: true
        Layout.maximumWidth: root.cardWidth
        Layout.alignment: Qt.AlignHCenter
        visible: root.syncBackendRef && root.syncBackendRef.manual_sync_pending
        text: qsTr("当前同步完成后将再次同步")
        color: Kirigami.Theme.disabledTextColor
        wrapMode: Text.Wrap
    }

    Label {
        id: syncConfigFeedback
        Layout.fillWidth: true
        Layout.maximumWidth: root.cardWidth
        Layout.alignment: Qt.AlignHCenter
        property string message: ""
        property bool isError: false
        visible: message.length > 0
        text: message
        color: isError ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.disabledTextColor
        wrapMode: Text.Wrap
    }

    Timer {
        id: configFeedbackTimer
        interval: 3000
        onTriggered: {
            syncConfigFeedback.message = ""
            syncConfigFeedback.visible = false
        }
    }

    // ── 结果/日志区 ──
    Rectangle {
        Layout.fillWidth: true
        Layout.maximumWidth: root.cardWidth
        Layout.alignment: Qt.AlignHCenter
        Layout.preferredHeight: 180
        color: Kirigami.Theme.alternateBackgroundColor
        border.color: root.isFailureStatus(root.currentSyncStatus) ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.disabledTextColor
        border.width: 1
        radius: Kirigami.Units.smallSpacing
        clip: true
        visible: (root.currentSyncOperationState !== "" || root.currentSyncStatus === "syncing" || root.isFailureStatus(root.currentSyncStatus))

        ScrollView {
            id: logScroll
            anchors.fill: parent
            anchors.margins: Kirigami.Units.smallSpacing
            ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
            ScrollBar.vertical.policy: ScrollBar.AsNeeded
            // Issue #782 评论 5855709706: 桌面鼠标左键不能按住空白处拖页面。
            Component.onCompleted: {
                if (contentItem) contentItem.acceptedButtons = Qt.NoButton
            }
            TextArea {
                id: syncResultArea
                width: logScroll.availableWidth
                text: root.resultText
                color: root.isFailureStatus(root.currentSyncStatus) ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.textColor
                font.family: "monospace"
                readOnly: true
                background: null
                wrapMode: TextEdit.Wrap
            }
        }
    }

    RowLayout {
        id: syncRawErrorRow
        Layout.fillWidth: true
        Layout.maximumWidth: root.cardWidth
        Layout.alignment: Qt.AlignHCenter
        visible: root.showRawError
        spacing: Kirigami.Units.smallSpacing

        Label {
            text: qsTr("诊断信息可复制")
            color: Kirigami.Theme.disabledTextColor
        }

        Button {
            text: qsTr("复制")
            flat: true
            onClicked: if (root.syncBackendRef) root.syncBackendRef.copy_text_to_clipboard(root.rawErrorText)
        }

        Item { Layout.fillWidth: true }
    }

    // Issue #762 评论 5826175490 第 4 点：跨作品冲突入口。
    // 按作品分组列出所有未解决冲突，点击直接跳到对应作品的冲突面板，
    // 不要求这一轮同步先结束。
    ColumnLayout {
        Layout.fillWidth: true
        Layout.maximumWidth: root.cardWidth
        Layout.alignment: Qt.AlignHCenter
        spacing: Kirigami.Units.smallSpacing
        visible: root.conflictRows.length > 0

        FormCard.FormHeader {
            title: qsTr("待处理冲突（%1 个作品 / %2 处）")
                .arg(root.conflictGroups.length)
                .arg(root.conflictTotalCount)
        }

        // Issue #762 评论 5826175490 第 4 点：conflictRows 已按
        // 「作品分组头 + 该作品每条冲突」展平，这里用单个 Repeater 渲染，
        // 分组头不可点击，冲突行点击后以作品 id + 冲突路径发出 openConflict。
        FormCard.FormCard {
            Repeater {
                model: root.conflictRows
                delegate: ColumnLayout {
                    Layout.fillWidth: true

                    FormCard.FormHeader {
                        visible: modelData.kind === "project"
                        // Issue #902 第二十轮：区分 App 级与作品级冲突标题。
                        title: modelData.targetKind === "app"
                            ? qsTr("应用数据/星图（%1 处）").arg(modelData.conflictCount)
                            : qsTr("%1（%2 处）").arg(modelData.projectTitle).arg(modelData.conflictCount)
                    }

                    FormCard.FormButtonDelegate {
                        visible: modelData.kind === "conflict"
                        text: modelData.localPath
                        onClicked: {
                            root.openConflict(modelData.targetKind, modelData.projectId, modelData.localPath)
                            // Issue #902 第二十轮：App 冲突在 SyncPage 内部展示面板，
                            // 不走 openWriting 跳转（main.qml 对 app target 直接 return）。
                            if (modelData.targetKind === "app") {
                                root.appConflictPath = modelData.localPath
                                appConflictPopup.open()
                            }
                        }
                    }
                    }
                }
            }
        }
    }

    // Issue #902 第二十一轮：App 冲突面板改为窗口级 Popup，挂在 Overlay.overlay 上。
    // 之前用 Loader { anchors.fill: parent } 铺满 SyncPage，但 SyncPage 是很长的
    // 设置内容项（implicitHeight = 全部同步设置高度），面板会跟随设置页 ScrollView
    // 滚动且被裁剪，Header 在顶部、解决按钮在底部都可能在可视区域外；面板根背景
    // 透明还会让底下设置控件透出来并被误触。改用 Popup 挂窗口级坐标系，模态层阻止
    // 底下设置项受误触，尺寸由 overlay 可用宽高约束，footer 始终可达。复用同一个
    // SyncConflictPanel（保持透明根背景供 RightDrawer 复用），背景归 Popup 容器负责。
    Popup {
        id: appConflictPopup
        modal: true
        focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        parent: Overlay.overlay
        // Issue #902 第二十二轮：删除硬最小宽高 400×300，小窗口（如 320×240）下
        // 居中后 x/y 会变负把面板挤出可见区域。改为不超过 overlay 实际可用尺寸的夹取，
        // 最小只保留 1 防止退化，绝不让最小高度反过来超过窗口。
        // Issue #900：原 resolvedDt.sp64/sp32 改用 Kirigami.Units.gridUnit 倍数。
        width: Math.max(1, Math.min(960, (parent ? parent.width : 960) - Kirigami.Units.gridUnit * 8))
        height: Math.max(1, Math.min(720, (parent ? parent.height : 720) - Kirigami.Units.gridUnit * 4))
        x: Math.round((parent.width - width) / 2)
        y: Math.round((parent.height - height) / 2)
        Overlay.modal: Rectangle { color: Qt.rgba(0, 0, 0, 0.32) }
        background: Rectangle {
            color: Kirigami.Theme.backgroundColor
            border.color: Kirigami.Theme.disabledTextColor
            border.width: 1.5
            radius: Kirigami.Units.largeSpacing
        }

        SyncConflictPanel {
            anchors.fill: parent
            // Issue #900：SyncConflictPanel 内部不再消费 dt 的 M3 字段，
            // 传 null 保持 required property API 满足。
            dt: null
            syncBackendRef: root.syncBackendRef
            targetKind: "app"
            projectId: ""
            conflicts: root.appConflictList
            requestedConflictPath: root.appConflictPath
            onCloseRequested: appConflictPopup.close()
            onConflictsResolved: {
                // 解决一个 App 冲突后刷新全局列表；conflicts 绑定到
                // appConflictList，refresh 后自动同步，无需手动赋值。
                root.refreshAllSyncConflicts()
            }
        }
    }

    Component.onCompleted: {
        if (root.syncBackendRef) {
            root.syncBackendRef.load_sync_config()
            autoSyncSwitch.checked = root.syncBackendRef.sync_auto_sync || false
            syncIntervalSpin.value = Math.round((root.syncBackendRef.sync_interval || 300) / 60)
            // Issue #762 评论 5826175490 第 4 点：页面打开时立即读取全局冲突，
            // 已有冲突不需要用户先手动同步一次才看得到。
            root.refreshAllSyncConflicts()
        }
        root.refreshLocalSyncState();
    }

    // Issue #762 评论 5826175490 第 2/5 点：SyncBackend 在同步开始前、每个 target
    // 结束、整轮同步结束、resolve 成功后都会发 sync_conflicts_changed。
    // 全局冲突入口据此实时刷新，"同步中"和"等待用户解决冲突"可以同时成立。
    Connections {
        target: root.syncBackendRef
        function onSync_conflicts_changed() {
            root.refreshAllSyncConflicts();
        }
    }
}
