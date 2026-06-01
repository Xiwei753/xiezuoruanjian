// =============================================================================
// AppController.qml — 应用状态控制器
// =============================================================================
//
// 层级：Linux UI 层（QML 逻辑控制器）
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
    property var projectBackendRef: null
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

    function generateActionId() {
        return Math.random().toString(36).substring(2, 8);
    }

    function emitError(message) {
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
        var projectApi = projectBackendRef || backendRef;
        if (!projectApi) return;
        var state = parseJson(projectApi.refresh_app_state_json(), fallbackMessage || qsTr("刷新应用状态失败"));
        if (state) applyState(state);
    }

    function restoreWorkspace() {
        var workspaceApi = workspaceBackendRef || backendRef;
        var appApi = appBackendRef || backendRef;
        if (!workspaceApi) return;
        if (appApi) appApi.query_system_color_scheme();
        workspaceApi.try_restore_last_workspace();
        refreshState(qsTr("恢复工作区失败"));
    }

    function openWriting(projectId, projectTitle) {
        writingProjectId = projectId || "";
        writingProjectTitle = projectTitle || qsTr("作品");
        starmapId = "";
        starmapTitle = "";
        route = "writing";
    }

    function openStarmap(id, title) {
        starmapId = id || "";
        starmapTitle = title || qsTr("星图编辑器");
        writingProjectId = "";
        writingProjectTitle = "";
        route = "starmap";
    }

    function openHub() {
        route = "hub";
        writingProjectId = "";
        writingProjectTitle = "";
        starmapId = "";
        starmapTitle = "";
        refreshState(qsTr("返回工作台失败"));
    }

    function switchWorkspace() {
        var workspaceApi = workspaceBackendRef || backendRef;
        if (!workspaceApi) return;
        workspaceApi.switch_workspace();
        route = "hub";
        refreshState(qsTr("切换工作区失败"));
    }

    function createWorkspace(openExisting) {
        var workspaceApi = workspaceBackendRef || backendRef;
        if (!workspaceApi) return;
        var rawResult = "";
        if (openExisting) {
            rawResult = workspaceApi.open_existing_workspace();
        } else {
            rawResult = workspaceApi.create_new_workspace();
        }
        var res = parseJson(rawResult, qsTr("操作失败"));
        if (!res) { 
            refreshState(qsTr("解析结果失败")); 
            return; 
        }
        if (res.success) {
            refreshState(qsTr("打开工作区成功"));
        } else {
            emitError(res.userMessage || res.message || qsTr("打开工作区失败"));
            refreshState(qsTr("打开工作区失败"));
        }
    }

    function createWorkspaceWithPath(path, openExisting) {
        var workspaceApi = workspaceBackendRef || backendRef;
        if (!workspaceApi) return;
        var rawResult = "";
        if (openExisting) {
            rawResult = workspaceApi.open_workspace_with_path(path);
        } else {
            rawResult = workspaceApi.create_workspace_with_path(path);
        }
        var res = parseJson(rawResult, qsTr("操作失败"));
        if (!res) { 
            refreshState(qsTr("解析结果失败")); 
            return; 
        }
        if (res.success) {
            refreshState(qsTr("打开工作区成功"));
        } else {
            emitError(res.userMessage || res.message || qsTr("打开工作区失败"));
            refreshState(qsTr("打开工作区失败"));
        }
    }

    function handleMutationResult(raw, fallbackMessage) {
        var res = parseJson(raw, fallbackMessage || qsTr("操作失败"));
        if (!res) return false;
        if (res.success) {
            if (res.state) applyState(res.state);
            else refreshState(qsTr("刷新操作结果失败"));
            return true;
        }
        emitError(res.userMessage || res.message || fallbackMessage || qsTr("操作失败"));
        return false;
    }

    function createProject(title) {
        var projectApi = projectBackendRef || backendRef;
        if (!projectApi) return false;
        var actionId = generateActionId();
        try {
            return handleMutationResult(projectApi.create_project_json(title, actionId), qsTr("创建作品失败"));
        } catch (e) {
            emitError(qsTr("后端调用失败: ") + e);
            return false;
        }
    }

    function renameProject(projectId, title) {
        var projectApi = projectBackendRef || backendRef;
        if (!projectApi || !projectId || !title) return false;
        try {
            projectApi.rename_project(projectId, title);
            refreshState(qsTr("刷新重命名结果失败"));
            return true;
        } catch (e) {
            emitError(qsTr("后端调用失败: ") + e);
            return false;
        }
    }

    function renameVolume(projectId, volumeId, title) {
        var projectApi = projectBackendRef || backendRef;
        if (!projectApi || !projectId || !volumeId || !title) return false;
        try {
            projectApi.rename_volume(projectId, volumeId, title);
            refreshState(qsTr("刷新重命名结果失败"));
            return true;
        } catch (e) {
            emitError(qsTr("后端调用失败: ") + e);
            return false;
        }
    }

    function renameChapter(projectId, volumeId, chapterId, title) {
        var projectApi = projectBackendRef || backendRef;
        if (!projectApi || !projectId || !volumeId || !chapterId || !title) return false;
        try {
            projectApi.rename_chapter(projectId, volumeId, chapterId, title);
            refreshState(qsTr("刷新重命名结果失败"));
            return true;
        } catch (e) {
            emitError(qsTr("后端调用失败: ") + e);
            return false;
        }
    }

    function createVolume(projectId, title) {
        var projectApi = projectBackendRef || backendRef;
        if (!projectApi) return false;
        var actionId = generateActionId();
        try {
            return handleMutationResult(projectApi.create_volume_json(projectId, title, actionId), qsTr("创建卷失败"));
        } catch (e) {
            emitError(qsTr("后端调用失败: ") + e);
            return false;
        }
    }

    function createChapter(projectId, volumeId, title) {
        var projectApi = projectBackendRef || backendRef;
        if (!projectApi) return false;
        var actionId = generateActionId();
        try {
            return handleMutationResult(projectApi.create_chapter_json(projectId, volumeId, title, actionId), qsTr("创建章节失败"));
        } catch (e) {
            emitError(qsTr("后端调用失败: ") + e);
            return false;
        }
    }

    function deleteItem(type, contextData) {
        var projectApi = projectBackendRef || backendRef;
        if (!projectApi) return false;
        var actionId = generateActionId();
        var raw = "";
        try {
            if (type === "delete_project") raw = projectApi.delete_project_json(contextData.projectId, actionId);
            else if (type === "delete_volume") raw = projectApi.delete_volume_json(contextData.projectId, contextData.volumeId, actionId);
            else if (type === "delete_chapter") raw = projectApi.delete_chapter_json(contextData.projectId, contextData.volumeId, contextData.chapterId, actionId);
            else return false;
            return handleMutationResult(raw, qsTr("删除失败"));
        } catch (e) {
            emitError(qsTr("后端调用失败: ") + e);
            return false;
        }
    }
}
