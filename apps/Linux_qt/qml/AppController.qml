// =============================================================================
// AppController.qml — 应用状态控制器
// =============================================================================
//
// 层级：Linux_qt UI 层（QML 逻辑控制器）
// 职责：管理全局路由状态、工作区状态、JSON 解析、错误处理
// 约束：
//   - 纯状态管理，不包含 UI 渲染
//   - 不直接调用 Core 层，通过 backendRef 委托
//   - route 状态驱动页面切换（hub / writing / starmap）
//
// 状态流：backendRef → appState → 各页面绑定
// =============================================================================

import QtQuick

QtObject {
    id: controller

    property var backendRef: null
    property var workspaceBackendRef: null
    property var stateBackendRef: null
    property var appBackendRef: null
    property var appState: ({
        hasWorkspace: false,
        workspacePath: "",
        saveStatus: "",
        selected: { projectId: "", volumeId: "", chapterId: "" },
        tree: [],
        settings: { fontSize: 16, themeMode: "dark" },
        sync: { status: "not_configured" }
    })
    property string route: "hub"
    property string writingProjectId: ""
    property string writingProjectTitle: ""
    property string starmapId: ""
    property string starmapTitle: ""
    property string errorMessage: ""

    readonly property bool inWriting: route === "writing"
    readonly property bool inStarmap: route === "starmap"

    signal errorRaised(string message)
    signal stateChanged(var state)

    property var refreshDebounceTimer: Timer {
        interval: 100
        repeat: false
        onTriggered: {
            var projectApi = stateBackendRef || appBackendRef || backendRef;
            if (!projectApi) return;
            var state = projectApi.refresh_app_state();
            if (state) applyState(state);
        }
    }

    function generateActionId() {
        return Math.random().toString(36).substring(2, 8);
    }

    function resolveMessageKey(messageKey) {
        switch (messageKey) {
        case "error.io": return qsTr("文件读写失败，请检查工作区权限和磁盘状态");
        case "error.json": return qsTr("数据文件格式异常，请检查工作区文件是否损坏");
        case "error.invalid_workspace": return qsTr("不是有效的工作区");
        case "error.project_not_found": return qsTr("作品不存在或已被删除");
        case "error.volume_not_found": return qsTr("卷不存在或已被删除");
        case "error.chapter_not_found": return qsTr("章节不存在或已被删除");
        case "error.empty_overwrite_blocked": return qsTr("已阻止空内容覆盖现有章节");
        case "error.not_implemented": return qsTr("该功能尚未实现");
        case "error.refuse_delete_workspace_root": return qsTr("拒绝删除工作区根目录");
        case "error.invalid_delete_target": return qsTr("删除目标无效");
        case "error.sync_conflict": return qsTr("同步冲突，请手动处理冲突文件后重试");
        case "error.sync_failed": return qsTr("同步失败，请检查网络和配置");
        case "error.other": return qsTr("操作失败");
        case "error.core_error": return qsTr("核心模块错误");
        case "error.clipboard_unavailable": return qsTr("复制失败：未找到可用的剪贴板后端");
        case "error.json_parse": return qsTr("数据解析失败");
        default: return qsTr("操作失败");
        }
    }

    function emitError(message) {
        if (message && message.startsWith("error.")) {
            message = resolveMessageKey(message);
        }
        errorMessage = message || qsTr("操作失败");
        errorRaised(errorMessage);
    }

    function parseJson(raw, fallbackMessage) {
        try {
            return JSON.parse(raw);
        } catch (e) {
            emitError((fallbackMessage || qsTr("解析后端返回数据失败")) + ": " + e);
            return null;
        }
    }

    function applyState(state) {
        if (!state) return;
        appState = state;
        stateChanged(appState);
    }

    function refreshState(fallbackMessage) {
        refreshDebounceTimer.restart();
    }

    function refreshStateImmediate(fallbackMessage) {
        var projectApi = stateBackendRef || appBackendRef || backendRef;
        if (!projectApi) return;
        var state = projectApi.refresh_app_state();
        if (state) applyState(state);
    }

    function restoreWorkspace() {
        var workspaceApi = workspaceBackendRef || backendRef;
        var appApi = appBackendRef || backendRef;
        if (!workspaceApi) return;
        if (appApi) appApi.query_system_color_scheme();
        workspaceApi.try_restore_last_workspace();
        refreshStateImmediate(qsTr("恢复工作区失败"));

        // 恢复上次导航状态
        if (workspaceApi.has_workspace) {
            var navState = workspaceApi.get_last_navigation_state();
            if (navState && navState.route) {
                var route = navState.route;
                if (route === "writing" && navState.projectId) {
                    // 验证 project 仍存在
                    var found = false;
                    if (appState && appState.tree) {
                        for (var i = 0; i < appState.tree.length; i++) {
                            if (appState.tree[i].id === navState.projectId) {
                                found = true;
                                break;
                            }
                        }
                    }
                    if (found) {
                        writingProjectId = navState.projectId;
                        writingProjectTitle = "";
                        starmapId = "";
                        starmapTitle = "";
                        controller.route = "writing";
                        // 如果有章节信息，设置选中章节
                        if (navState.volumeId && navState.chapterId && editorBackend) {
                            editorBackend.selected_project_id = navState.projectId;
                            editorBackend.selected_volume_id = navState.volumeId;
                            editorBackend.selected_chapter_id = navState.chapterId;
                        }
                    } else {
                        // 章节不存在，退回 hub
                        controller.route = "hub";
                    }
                } else if (route === "starmap" && navState.starmapId) {
                    starmapId = navState.starmapId;
                    starmapTitle = "";
                    writingProjectId = "";
                    writingProjectTitle = "";
                    controller.route = "starmap";
                } else {
                    controller.route = "hub";
                }
            }
        }
    }

    function saveNavigationState() {
        var workspaceApi = workspaceBackendRef || backendRef;
        if (!workspaceApi) return;
        var projectId = writingProjectId || "";
        var volumeId = (editorBackend && editorBackend.selected_volume_id) ? editorBackend.selected_volume_id : "";
        var chapterId = (editorBackend && editorBackend.selected_chapter_id) ? editorBackend.selected_chapter_id : "";
        var smId = starmapId || "";
        workspaceApi.save_last_navigation_state(route, projectId, volumeId, chapterId, smId);
    }

    function openWriting(projectId, projectTitle) {
        writingProjectId = projectId || "";
        writingProjectTitle = projectTitle || qsTr("作品");
        starmapId = "";
        starmapTitle = "";
        route = "writing";
        saveNavigationState();
    }

    function openStarmap(id, title) {
        starmapId = id || "";
        starmapTitle = title || qsTr("星图编辑器");
        writingProjectId = "";
        writingProjectTitle = "";
        route = "starmap";
        saveNavigationState();
    }

    function openHub() {
        route = "hub";
        writingProjectId = "";
        writingProjectTitle = "";
        starmapId = "";
        starmapTitle = "";
        saveNavigationState();
        refreshStateImmediate(qsTr("返回工作台失败"));
    }

    function switchWorkspace() {
        var workspaceApi = workspaceBackendRef || backendRef;
        if (!workspaceApi) return;
        workspaceApi.switch_workspace();
        route = "hub";
        workspaceApi.clear_last_navigation_state();
        refreshStateImmediate(qsTr("切换工作区失败"));
    }

    function createWorkspace(openExisting) {
        var workspaceApi = workspaceBackendRef || backendRef;
        if (!workspaceApi) return;
        var res = null;
        if (openExisting) {
            res = workspaceApi.open_existing_workspace();
        } else {
            res = workspaceApi.create_new_workspace();
        }
        if (!res) { 
            refreshStateImmediate(qsTr("解析结果失败")); 
            return; 
        }
        if (res.success) {
            refreshStateImmediate(qsTr("打开工作区成功"));
        } else {
            emitError(qsTr("打开工作区失败"));
            refreshStateImmediate(qsTr("打开工作区失败"));
        }
    }

    function createWorkspaceWithPath(path, openExisting) {
        var workspaceApi = workspaceBackendRef || backendRef;
        if (!workspaceApi) return;
        var res = null;
        if (openExisting) {
            res = workspaceApi.open_workspace_with_path(path);
        } else {
            res = workspaceApi.create_workspace_with_path(path);
        }
        if (!res) { 
            refreshStateImmediate(qsTr("解析结果失败")); 
            return; 
        }
        if (res.success) {
            refreshStateImmediate(qsTr("打开工作区成功"));
        } else {
            emitError(qsTr("打开工作区失败"));
            refreshStateImmediate(qsTr("打开工作区失败"));
        }
    }

    function handleMutationResult(res, fallbackMessage) {
        if (!res) return false;
        if (res.success) {
            if (res.state) applyState(res.state);
            else refreshStateImmediate(qsTr("刷新操作结果失败"));
            return true;
        }
        emitError(fallbackMessage || qsTr("操作失败"));
        return false;
    }

}
